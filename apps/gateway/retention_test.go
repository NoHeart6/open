package main

import (
	"testing"
	"time"
)

func TestOutputRetentionPreservesRunningAndRecentTasks(t *testing.T) {
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
	if err := s.pruneOutput(now); err != nil {
		t.Fatal(err)
	}
	for id, expected := range map[string]int{"old-finished": 0, "recent-finished": 1, "old-running": 1} {
		var count int
		if err := s.db.QueryRow("SELECT COUNT(*) FROM task_output WHERE task_id=?", id).Scan(&count); err != nil {
			t.Fatal(err)
		}
		if count != expected {
			t.Fatalf("%s: retained %d, want %d", id, count, expected)
		}
		if _, err := s.task("alice", id); err != nil {
			t.Fatalf("retention must preserve task identity: %v", err)
		}
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
