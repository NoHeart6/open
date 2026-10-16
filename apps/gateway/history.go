package main

import "time"

const historyIdleSeconds int64 = 3600

func (s *Server) touchHistory(owner string, at int64) error {
	_, err := s.db.Exec(`INSERT INTO mcp_activity(owner,last_used) VALUES(?,?) ON CONFLICT(owner) DO UPDATE SET last_used=MAX(last_used,excluded.last_used)`, owner, at)
	return err
}

func (s *Server) historyActivity(owner string) (int64, error) {
	var at int64
	err := s.db.QueryRow("SELECT COALESCE((SELECT last_used FROM mcp_activity WHERE owner=?),0)", owner).Scan(&at)
	return at, err
}

func (s *Server) pruneHistory(now int64) error {
	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()
	_, err = tx.Exec(`CREATE TEMP TABLE IF NOT EXISTS idle_owners(owner TEXT PRIMARY KEY)`)
	if err != nil {
		return err
	}
	if _, err = tx.Exec("DELETE FROM idle_owners"); err != nil {
		return err
	}
	_, err = tx.Exec(`INSERT INTO idle_owners SELECT owner FROM mcp_activity a WHERE last_used<=? AND NOT EXISTS
		(SELECT 1 FROM tasks t WHERE t.owner=a.owner AND t.deadline>? AND t.status IN ('queued','dispatched','received','running','approval_pending','cancelling','uncertain'))`, now-historyIdleSeconds, now)
	if err != nil {
		return err
	}
	for _, query := range []string{
		"DELETE FROM task_output WHERE task_id IN (SELECT id FROM tasks WHERE owner IN (SELECT owner FROM idle_owners))",
		"DELETE FROM audit WHERE owner IN (SELECT owner FROM idle_owners)",
		"DELETE FROM tasks WHERE owner IN (SELECT owner FROM idle_owners)",
	} {
		if _, err = tx.Exec(query); err != nil {
			return err
		}
	}
	return tx.Commit()
}

func (s *Server) recordHistoryActivity(owner string) error {
	return s.touchHistory(owner, time.Now().Unix())
}
