package tasks_test

import (
	"testing"

	"opencommander/gateway/internal/tasks"
)

func TestIdempotency(t *testing.T) {
	s := tasks.NewStore()
	a := s.CreateTask("o1", "d1", "system_info", nil, "k1")
	b := s.CreateTask("o1", "d1", "system_info", nil, "k1")
	if a.ID != b.ID {
		t.Fatalf("idempotency key must return same task")
	}
}

func TestCancel(t *testing.T) {
	s := tasks.NewStore()
	a := s.CreateTask("o1", "d1", "system_info", nil, "")
	if !s.Cancel(a.ID) {
		t.Fatalf("cancel should succeed")
	}
	if s.Cancel(a.ID) {
		t.Fatalf("second cancel must fail")
	}
}

func TestPairing(t *testing.T) {
	s := tasks.NewStore()
	code := s.NewPairingCode("owner1")
	owner, ok := s.ClaimDevice(code, "dev1")
	if !ok || owner != "owner1" {
		t.Fatalf("claim failed")
	}
	if _, ok := s.ClaimDevice(code, "dev1"); ok {
		t.Fatalf("code reuse must fail")
	}
}
