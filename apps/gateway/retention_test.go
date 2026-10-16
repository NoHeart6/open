package main

import (
	"testing"
	"time"
)

func TestIdleHistoryCleanupWaitsForOneHourAndActiveTasks(t *testing.T) {
	s, h := fixture(t)
	device, _ := pair(t, s, h)
	now := time.Now().Unix()
	for _, task := range []struct {
		id, status string
		at         int64
	}{
		{"old-finished", "succeeded", now - 8*24*60*60},
		{"recent-finished", "failed", now - 60},
		{"old-running", "running", now - 8*24*60*60},
	} {
		_, err := s.db.Exec(`INSERT INTO tasks(id,owner,device_id,action,args,idempotency_key,fingerprint,status,category,created_at,deadline) VALUES(?,'alice',?,'terminal_execute','{}',?,'test',?,'SENSITIVE',?,?)`, task.id, device, task.id, task.status, task.at, now+60)
		if err != nil {
			t.Fatal(err)
		}
		_, err = s.db.Exec("INSERT INTO task_output(task_id,seq,stream,text,at) VALUES(?,1,'stdout','fixture',?)", task.id, task.at)
		if err != nil {
			t.Fatal(err)
		}
	}
	if _, err := s.db.Exec("UPDATE mcp_activity SET last_used=? WHERE owner='alice'", now-3599); err != nil {
		t.Fatal(err)
	}
	if err := s.pruneHistory(now); err != nil {
		t.Fatal(err)
	}
	for id, expected := range map[string]int{"old-finished": 1, "recent-finished": 1, "old-running": 1} {
		var count int
		if err := s.db.QueryRow("SELECT COUNT(*) FROM task_output WHERE task_id=?", id).Scan(&count); err != nil {
			t.Fatal(err)
		}
		if count != expected {
			t.Fatalf("%s: retained %d, want %d", id, count, expected)
		}
	}
	if _, err := s.db.Exec("UPDATE mcp_activity SET last_used=? WHERE owner='alice'", now-3600); err != nil {
		t.Fatal(err)
	}
	if err := s.pruneHistory(now); err != nil {
		t.Fatal(err)
	}
	if _, err := s.task("alice", "old-finished"); err != nil {
		t.Fatal("active task must defer all history cleanup", err)
	}
	if _, err := s.db.Exec("UPDATE tasks SET status='succeeded' WHERE id='old-running'"); err != nil {
		t.Fatal(err)
	}
	if err := s.pruneHistory(now); err != nil {
		t.Fatal(err)
	}
	for _, table := range []string{"tasks", "task_output", "audit"} {
		var count int
		if err := s.db.QueryRow("SELECT COUNT(*) FROM " + table).Scan(&count); err != nil {
			t.Fatal(err)
		}
		if count != 0 {
			t.Fatalf("%s retained %d history rows", table, count)
		}
	}
	if !s.owned("alice", device) {
		t.Fatal("cleanup removed pairing")
	}
}

func TestMCPReadsResetIdleHistoryAndOtherAccountsDoNot(t *testing.T) {
	s, h := fixture(t)
	device, _ := pair(t, s, h)
	_, result := request(t, s, h, "POST", "/api/tasks", "alice", taskBody(device, "idle-read-test"))
	id := result["task_id"].(string)
	now := time.Now().Unix()
	if _, err := s.db.Exec("UPDATE tasks SET status='succeeded' WHERE id=?", id); err != nil {
		t.Fatal(err)
	}
	if _, err := s.db.Exec("UPDATE mcp_activity SET last_used=? WHERE owner='alice'", now-3600); err != nil {
		t.Fatal(err)
	}
	if code, _ := request(t, s, h, "GET", "/api/tasks/"+id, "alice", nil); code != 200 {
		t.Fatal(code)
	}
	if err := s.pruneHistory(now); err != nil {
		t.Fatal(err)
	}
	if _, err := s.task("alice", id); err != nil {
		t.Fatal("status polling did not reset inactivity", err)
	}
	if _, err := s.db.Exec("UPDATE mcp_activity SET last_used=? WHERE owner='alice'", now-3600); err != nil {
		t.Fatal(err)
	}
	request(t, s, h, "GET", "/api/devices", "mallory", nil)
	if err := s.pruneHistory(now); err != nil {
		t.Fatal(err)
	}
	if _, err := s.task("alice", id); err == nil {
		t.Fatal("another account extended retention")
	}
}

func TestOutputCursorWaitsForOutOfOrderChunks(t *testing.T) {
	s, h := fixture(t)
	device, _ := pair(t, s, h)
	_, err := s.db.Exec(`INSERT INTO tasks(id,owner,device_id,action,args,idempotency_key,fingerprint,status,category,created_at,deadline) VALUES('output-fixture','alice',?,'terminal_execute','{}','output-fixture','test','running','SENSITIVE',0,9999999999)`, device)
	if err != nil {
		t.Fatal(err)
	}
	_, err = s.db.Exec("INSERT INTO task_output(task_id,seq,stream,text,at) VALUES('output-fixture',2,'stderr','second',?)", time.Now().Unix())
	if err != nil {
		t.Fatal(err)
	}
	code, result := request(t, s, h, "GET", "/api/tasks/output-fixture/output?after=0", "alice", nil)
	if code != 200 || result["next_after"] != float64(0) || len(result["chunks"].([]any)) != 0 {
		t.Fatalf("cursor crossed missing chunk: %d %v", code, result)
	}
	_, err = s.db.Exec("INSERT INTO task_output(task_id,seq,stream,text,at) VALUES('output-fixture',1,'stdout','first',?)", time.Now().Unix())
	if err != nil {
		t.Fatal(err)
	}
	code, result = request(t, s, h, "GET", "/api/tasks/output-fixture/output?after=0", "alice", nil)
	if code != 200 || result["next_after"] != float64(2) || len(result["chunks"].([]any)) != 2 {
		t.Fatalf("cursor lost delayed chunk: %d %v", code, result)
	}
}
