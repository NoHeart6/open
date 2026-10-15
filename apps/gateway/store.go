package main

import (
	"database/sql"
	"encoding/json"
	"fmt"
	_ "modernc.org/sqlite"
	"os"
	"time"
)

const migration = `
CREATE TABLE IF NOT EXISTS devices(id TEXT PRIMARY KEY, owner TEXT NOT NULL, public_key TEXT NOT NULL, name TEXT NOT NULL, revoked INTEGER NOT NULL DEFAULT 0, last_seen INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS enrollments(id TEXT PRIMARY KEY, code_hash TEXT UNIQUE NOT NULL, secret_hash TEXT NOT NULL, public_key TEXT NOT NULL, name TEXT NOT NULL, expires INTEGER NOT NULL, owner TEXT, consumed INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS tasks(id TEXT PRIMARY KEY, owner TEXT NOT NULL, device_id TEXT NOT NULL REFERENCES devices(id), action TEXT NOT NULL, args TEXT NOT NULL, idempotency_key TEXT NOT NULL, fingerprint TEXT NOT NULL, status TEXT NOT NULL, category TEXT NOT NULL, created_at INTEGER NOT NULL, started_at INTEGER, completed_at INTEGER, deadline INTEGER NOT NULL, result TEXT, error TEXT, UNIQUE(owner, device_id, idempotency_key));
CREATE TABLE IF NOT EXISTS audit(id INTEGER PRIMARY KEY, at INTEGER NOT NULL, owner TEXT NOT NULL, device_id TEXT NOT NULL, task_id TEXT NOT NULL, event TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS nonces(nonce TEXT PRIMARY KEY, expires INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS revoked_tokens(jti TEXT PRIMARY KEY, expires INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS task_output(task_id TEXT NOT NULL REFERENCES tasks(id), seq INTEGER NOT NULL, stream TEXT NOT NULL, text TEXT NOT NULL, at INTEGER NOT NULL, PRIMARY KEY(task_id,seq));
CREATE INDEX IF NOT EXISTS task_output_retention ON task_output(at);
PRAGMA user_version=1;
`

func (s *Server) pruneOutput(now int64) error {
	// Preserve chunks of unfinished work; finished output has a seven-day retention.
	_, err := s.db.Exec(`DELETE FROM task_output WHERE at<? AND task_id IN
		(SELECT id FROM tasks WHERE status IN ('succeeded','failed','cancelled','expired','uncertain'))`, now-7*24*60*60)
	return err
}

func openStore(path string) (*sql.DB, error) {
	db, err := sql.Open("sqlite", path)
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	if _, err = db.Exec("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;" + migration); err != nil {
		db.Close()
		return nil, err
	}
	if err = os.Chmod(path, 0600); err != nil {
		db.Close()
		return nil, err
	}
	// A dispatched operation might have reached the PC. Never replay it after a restart.
	_, err = db.Exec("UPDATE tasks SET status='uncertain', error='gateway restarted before outcome was recorded' WHERE status IN ('dispatched','running','approval_pending','cancelling')")
	return db, err
}

type Task struct {
	ID              string          `json:"task_id"`
	Owner           string          `json:"owner"`
	DeviceID        string          `json:"device_id"`
	Action          string          `json:"action"`
	Args            json.RawMessage `json:"args"`
	Status          string          `json:"status"`
	Category        string          `json:"category"`
	CreatedAt       int64           `json:"created_at"`
	StartedAt       sql.NullInt64   `json:"-"`
	CompletedAt     sql.NullInt64   `json:"-"`
	StartedAtUnix   *int64          `json:"started_at"`
	CompletedAtUnix *int64          `json:"completed_at"`
	RetryEligible   bool            `json:"retry_eligible"`
	Deadline        int64           `json:"deadline"`
	Result          json.RawMessage `json:"result,omitempty"`
	Error           string          `json:"error,omitempty"`
}

func (s *Server) task(owner, id string) (*Task, error) {
	t := &Task{}
	var args string
	var result, taskError sql.NullString
	err := s.db.QueryRow(`SELECT id,owner,device_id,action,args,status,category,created_at,started_at,completed_at,deadline,result,error FROM tasks WHERE id=? AND owner=?`, id, owner).Scan(&t.ID, &t.Owner, &t.DeviceID, &t.Action, &args, &t.Status, &t.Category, &t.CreatedAt, &t.StartedAt, &t.CompletedAt, &t.Deadline, &result, &taskError)
	t.Args = json.RawMessage(args)
	if result.Valid {
		t.Result = json.RawMessage(result.String)
	}
	t.Error = taskError.String
	if t.StartedAt.Valid {
		t.StartedAtUnix = &t.StartedAt.Int64
	}
	if t.CompletedAt.Valid {
		t.CompletedAtUnix = &t.CompletedAt.Int64
	}
	t.RetryEligible = t.Category == "READ"
	return t, err
}

func (s *Server) audit(owner, device, task, event string) error {
	_, err := s.db.Exec("INSERT INTO audit(at,owner,device_id,task_id,event) VALUES(?,?,?,?,?)", time.Now().Unix(), owner, device, task, event)
	return err
}

func category(action string, args json.RawMessage) (string, error) {
	switch action {
	case "system_info", "system_inventory", "file_read", "file_search", "desktop_windows", "desktop_screenshot", "desktop_inspect":
		return "READ", nil
	case "file_write", "file_batch", "file_undo":
		return "MODIFY", nil
	case "file_recovery":
		var a struct {
			Op string `json:"op"`
		}
		if json.Unmarshal(args, &a) != nil {
			return "", fmt.Errorf("invalid recovery operation")
		}
		if a.Op == "list" {
			return "READ", nil
		}
		if a.Op == "restore" {
			return "MODIFY", nil
		}
	case "file_operation":
		var a struct {
			Op string `json:"op"`
		}
		if err := json.Unmarshal(args, &a); err != nil {
			return "", err
		}
		switch a.Op {
		case "list", "metadata", "hash", "disk_usage":
			return "READ", nil
		case "mkdir", "copy", "move", "rename", "edit", "zip", "unzip":
			return "MODIFY", nil
		case "delete":
			return "SENSITIVE", nil
		}
	case "process_manage":
		var a struct {
			Op string `json:"op"`
		}
		if err := json.Unmarshal(args, &a); err != nil {
			return "", err
		}
		if a.Op == "list" {
			return "READ", nil
		}
		if a.Op == "terminate" {
			return "SENSITIVE", nil
		}
	case "image_place", "wordcraft", "terminal_execute", "terminal_input", "application_launch", "desktop_interact", "desktop_automation", "clipboard_access":
		return "SENSITIVE", nil
	case "terminal_screen":
		return "READ", nil
	}
	return "", fmt.Errorf("unsupported action or operation")
}
