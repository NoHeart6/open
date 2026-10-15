package main

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"database/sql"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/gorilla/websocket"
)

type Config struct {
	Addr, PublicAddr, Database, BridgeKey, SigningSeed string
	Dev                                                bool
}
type Session struct {
	conn *websocket.Conn
	out  chan any
	done chan struct{}
}
type bucket struct {
	count int
	reset time.Time
}
type Server struct {
	db       *sql.DB
	cfg      Config
	signer   ed25519.PrivateKey
	mu       sync.Mutex
	sessions map[string]*Session
	limits   map[string]bucket
	ctx      context.Context
	cancel   context.CancelFunc
}
type Claims struct {
	jwt.RegisteredClaims
	Scope        string `json:"scope"`
	TokenID      string `json:"token_jti"`
	TokenExpires int64  `json:"token_expires"`
}

func newServer(cfg Config) (*Server, error) {
	if len(cfg.BridgeKey) < 32 {
		return nil, errors.New("BRIDGE_KEY must contain at least 32 bytes")
	}
	seed, err := base64.StdEncoding.DecodeString(cfg.SigningSeed)
	if err != nil || len(seed) != 32 {
		return nil, errors.New("GATEWAY_SIGNING_SEED must be a base64 Ed25519 32-byte seed")
	}
	db, err := openStore(cfg.Database)
	if err != nil {
		return nil, err
	}
	ctx, cancel := context.WithCancel(context.Background())
	return &Server{db: db, cfg: cfg, signer: ed25519.NewKeyFromSeed(seed), sessions: map[string]*Session{}, limits: map[string]bucket{}, ctx: ctx, cancel: cancel}, nil
}
func token(n int) string {
	b := make([]byte, n)
	if _, err := rand.Read(b); err != nil {
		panic(err)
	}
	return base64.RawURLEncoding.EncodeToString(b)
}
func digest(v string) string { h := sha256.Sum256([]byte(v)); return hex.EncodeToString(h[:]) }
func respond(w http.ResponseWriter, code int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Cache-Control", "no-store")
	w.WriteHeader(code)
	_ = json.NewEncoder(w).Encode(v)
}
func fail(w http.ResponseWriter, code int, msg string) {
	respond(w, code, map[string]any{"error": map[string]string{"code": http.StatusText(code), "message": msg}})
}
func decode(w http.ResponseWriter, r *http.Request, v any) bool {
	r.Body = http.MaxBytesReader(w, r.Body, 256*1024)
	d := json.NewDecoder(r.Body)
	d.DisallowUnknownFields()
	if err := d.Decode(v); err != nil {
		fail(w, 400, "invalid or oversized JSON body")
		return false
	}
	var trailing any
	if d.Decode(&trailing) != io.EOF {
		fail(w, 400, "trailing JSON data")
		return false
	}
	return true
}
func (s *Server) allowed(key string, max int) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	now := time.Now()
	if len(s.limits) > 10000 {
		for k, v := range s.limits {
			if now.After(v.reset) {
				delete(s.limits, k)
			}
		}
		if len(s.limits) > 10000 {
			return false
		}
	}
	b := s.limits[key]
	if now.After(b.reset) {
		b = bucket{reset: now.Add(time.Minute)}
	}
	b.count++
	s.limits[key] = b
	return b.count <= max
}
func (s *Server) authenticate(w http.ResponseWriter, r *http.Request) (*Claims, bool) {
	raw := strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")
	if raw == r.Header.Get("Authorization") {
		fail(w, 401, "Bearer assertion required")
		return nil, false
	}
	claims := &Claims{}
	_, err := jwt.ParseWithClaims(raw, claims, func(t *jwt.Token) (any, error) { return []byte(s.cfg.BridgeKey), nil }, jwt.WithValidMethods([]string{"HS256"}), jwt.WithIssuer("opencommander-bridge"), jwt.WithAudience("opencommander-gateway"), jwt.WithExpirationRequired(), jwt.WithIssuedAt())
	if err != nil || claims.Subject == "" || claims.IssuedAt == nil || claims.ExpiresAt.Time.Sub(claims.IssuedAt.Time) > 30*time.Second {
		fail(w, 401, "invalid internal identity assertion")
		return nil, false
	}
	var revoked int
	if err := s.db.QueryRow("SELECT count(*) FROM revoked_tokens WHERE jti=? AND expires>?", claims.TokenID, time.Now().Unix()).Scan(&revoked); err != nil {
		fail(w, 503, "revocation storage unavailable")
		return nil, false
	}
	if revoked != 0 {
		fail(w, 401, "token revoked")
		return nil, false
	}
	if !s.allowed("owner:"+claims.Subject, 300) {
		fail(w, 429, "account rate limit exceeded")
		return nil, false
	}
	return claims, true
}
func hasScope(scope, wanted string) bool {
	for _, v := range strings.Fields(scope) {
		if v == wanted {
			return true
		}
	}
	return false
}
func (s *Server) owned(owner, id string) bool {
	var n int
	return s.db.QueryRow("SELECT count(*) FROM devices WHERE id=? AND owner=? AND revoked=0", id, owner).Scan(&n) == nil && n == 1
}
func (s *Server) handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /healthz", func(w http.ResponseWriter, r *http.Request) {
		if err := s.db.Ping(); err != nil {
			fail(w, 503, "storage unavailable")
			return
		}
		respond(w, 200, map[string]string{"status": "ok", "protocol": "1"})
	})
	mux.HandleFunc("POST /v1/enroll", s.enroll)
	mux.HandleFunc("POST /v1/enroll/{id}/poll", s.pollEnrollment)
	mux.HandleFunc("GET /v1/agent", s.connectAgent)
	mux.HandleFunc("/api/", func(w http.ResponseWriter, r *http.Request) {
		c, ok := s.authenticate(w, r)
		if ok {
			s.api(w, r, c)
		}
	})
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if s.ctx.Err() != nil {
			fail(w, 503, "gateway shutting down")
			return
		}
		w.Header().Set("X-Content-Type-Options", "nosniff")
		mux.ServeHTTP(w, r)
	})
}

// The Railway edge targets this listener. Identity assertions remain on the private listener.
func (s *Server) publicHandler() http.Handler {
	internal := s.handler()
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/healthz" && !strings.HasPrefix(r.URL.Path, "/v1/") {
			fail(w, http.StatusNotFound, "route not found")
			return
		}
		internal.ServeHTTP(w, r)
	})
}
func (s *Server) enroll(w http.ResponseWriter, r *http.Request) {
	ip, _, _ := net.SplitHostPort(r.RemoteAddr)
	if !s.allowed("enroll:"+ip, 10) {
		fail(w, 429, "enrollment rate limit")
		return
	}
	var a struct {
		PublicKey string `json:"public_key"`
		Name      string `json:"name"`
	}
	if !decode(w, r, &a) {
		return
	}
	key, err := base64.StdEncoding.DecodeString(a.PublicKey)
	if err != nil || len(key) != 32 || len(a.Name) > 100 || a.Name == "" {
		fail(w, 400, "invalid device identity")
		return
	}
	id, secret, code := token(18), token(32), strings.ToUpper(hex.EncodeToString(randomBytes(8)))
	expires := time.Now().Add(5 * time.Minute).Unix()
	_, err = s.db.Exec("INSERT INTO enrollments(id,code_hash,secret_hash,public_key,name,expires) VALUES(?,?,?,?,?,?)", id, digest(code), digest(secret), a.PublicKey, a.Name, expires)
	if err != nil {
		fail(w, 500, "enrollment storage failed")
		return
	}
	respond(w, 201, map[string]any{"enrollment_id": id, "pairing_code": code, "poll_secret": secret, "expires_at": expires})
}
func randomBytes(n int) []byte {
	b := make([]byte, n)
	if _, err := rand.Read(b); err != nil {
		panic(err)
	}
	return b
}
func (s *Server) pollEnrollment(w http.ResponseWriter, r *http.Request) {
	ip, _, _ := net.SplitHostPort(r.RemoteAddr)
	if !s.allowed("poll:"+ip, 60) {
		fail(w, 429, "poll rate limit")
		return
	}
	var a struct {
		Secret string `json:"secret"`
	}
	if !decode(w, r, &a) {
		return
	}
	var owner sql.NullString
	var hash string
	var expires int64
	err := s.db.QueryRow("SELECT owner,secret_hash,expires FROM enrollments WHERE id=?", r.PathValue("id")).Scan(&owner, &hash, &expires)
	if err != nil || subtle.ConstantTimeCompare([]byte(hash), []byte(digest(a.Secret))) != 1 || expires < time.Now().Unix() {
		fail(w, 403, "invalid or expired enrollment")
		return
	}
	if !owner.Valid {
		respond(w, 202, map[string]string{"status": "pending"})
		return
	}
	respond(w, 200, map[string]any{"device_id": r.PathValue("id"), "owner": owner.String, "gateway_public_key": base64.StdEncoding.EncodeToString(s.signer.Public().(ed25519.PublicKey))})
}
func (s *Server) api(w http.ResponseWriter, r *http.Request, c *Claims) {
	if !hasScope(c.Scope, "computer:read") {
		fail(w, 403, "computer:read scope required")
		return
	}
	path := strings.TrimPrefix(r.URL.Path, "/api/")
	switch {
	case path == "token/revoke" && r.Method == "POST":
		if c.TokenID == "" || c.TokenExpires <= time.Now().Unix() {
			fail(w, 400, "missing token expiry assertion")
			return
		}
		if _, err := s.db.Exec("INSERT OR REPLACE INTO revoked_tokens(jti,expires) VALUES(?,?)", c.TokenID, c.TokenExpires); err != nil {
			fail(w, 500, "revocation failed")
			return
		}
		respond(w, 200, map[string]string{"status": "revoked"})
	case path == "pair/claim" && r.Method == "POST":
		if !hasScope(c.Scope, "computer:control") {
			fail(w, 403, "control scope required")
			return
		}
		var a struct {
			Code string `json:"code"`
		}
		if !decode(w, r, &a) {
			return
		}
		if !s.allowed("claim:"+c.Subject, 5) {
			fail(w, 429, "pairing attempts exceeded")
			return
		}
		tx, err := s.db.Begin()
		if err != nil {
			fail(w, 500, "storage failed")
			return
		}
		defer tx.Rollback()
		var id, key, name string
		err = tx.QueryRow("SELECT id,public_key,name FROM enrollments WHERE code_hash=? AND consumed=0 AND expires>?", digest(strings.ToUpper(a.Code)), time.Now().Unix()).Scan(&id, &key, &name)
		if err != nil {
			fail(w, 403, "invalid or expired pairing code")
			return
		}
		if _, err = tx.Exec("UPDATE enrollments SET owner=?,consumed=1 WHERE id=?", c.Subject, id); err == nil {
			_, err = tx.Exec("INSERT INTO devices(id,owner,public_key,name) VALUES(?,?,?,?)", id, c.Subject, key, name)
		}
		if err == nil {
			_, err = tx.Exec("INSERT INTO audit(at,owner,device_id,task_id,event) VALUES(?,?,?,'','paired')", time.Now().Unix(), c.Subject, id)
		}
		if err != nil || tx.Commit() != nil {
			fail(w, 500, "pairing failed")
			return
		}
		respond(w, 200, map[string]string{"device_id": id, "owner": c.Subject})
	case path == "devices" && r.Method == "GET":
		if s.audit(c.Subject, "", "", "device_list") != nil {
			fail(w, 503, "audit unavailable")
			return
		}
		rows, err := s.db.Query("SELECT id,name,last_seen FROM devices WHERE owner=? AND revoked=0", c.Subject)
		if err != nil {
			fail(w, 500, "storage failed")
			return
		}
		defer rows.Close()
		devices := []any{}
		for rows.Next() {
			var id, name string
			var seen int64
			if rows.Scan(&id, &name, &seen) != nil {
				fail(w, 500, "device read failed")
				return
			}
			s.mu.Lock()
			_, online := s.sessions[id]
			s.mu.Unlock()
			devices = append(devices, map[string]any{"device_id": id, "name": name, "last_seen": seen, "connected": online && time.Now().Unix()-seen < 45})
		}
		respond(w, 200, map[string]any{"devices": devices})
	case strings.HasPrefix(path, "devices/") && r.Method == "DELETE":
		if !hasScope(c.Scope, "computer:control") {
			fail(w, 403, "control scope required")
			return
		}
		id := strings.TrimPrefix(path, "devices/")
		if !s.owned(c.Subject, id) {
			fail(w, 404, "device not found")
			return
		}
		_, err := s.db.Exec("UPDATE devices SET revoked=1 WHERE id=? AND owner=?", id, c.Subject)
		if err != nil {
			fail(w, 500, "revoke failed")
			return
		}
		_, _ = s.db.Exec("UPDATE tasks SET status='cancelled',completed_at=?,error='device revoked' WHERE device_id=? AND status='queued'", time.Now().Unix(), id)
		s.mu.Lock()
		if sess := s.sessions[id]; sess != nil {
			sess.conn.Close()
		}
		s.mu.Unlock()
		_ = s.audit(c.Subject, id, "", "revoked")
		respond(w, 200, map[string]string{"status": "revoked"})
	case path == "tasks" && r.Method == "POST":
		s.createTask(w, r, c)
	case strings.HasPrefix(path, "tasks/"):
		id := strings.TrimPrefix(path, "tasks/")
		cancel := strings.HasSuffix(id, "/cancel")
		output := strings.HasSuffix(id, "/output")
		id = strings.TrimSuffix(id, "/cancel")
		id = strings.TrimSuffix(id, "/output")
		t, err := s.task(c.Subject, id)
		if err != nil {
			fail(w, 404, "task not found")
			return
		}
		if s.audit(c.Subject, t.DeviceID, t.ID, "task_read") != nil {
			fail(w, 503, "audit unavailable")
			return
		}
		if output && r.Method == "GET" {
			var after int64
			if raw := r.URL.Query().Get("after"); raw != "" {
				if _, err := fmt.Sscan(raw, &after); err != nil || after < 0 {
					fail(w, 400, "invalid output cursor")
					return
				}
			}
			rows, err := s.db.Query("SELECT seq,stream,text FROM task_output WHERE task_id=? AND seq>? ORDER BY seq LIMIT 64", id, after)
			if err != nil {
				fail(w, 500, "output read failed")
				return
			}
			defer rows.Close()
			chunks := []any{}
			for rows.Next() {
				var seq int64
				var stream, text string
				if rows.Scan(&seq, &stream, &text) != nil {
					fail(w, 500, "output read failed")
					return
				}
				// stdout/stderr and reconnect retransmissions can arrive out of order.
				// Never advance a cursor across a chunk that has not been committed yet.
				if seq != after+1 {
					break
				}
				chunks = append(chunks, map[string]any{"seq": seq, "stream": stream, "text": text})
				after = seq
			}
			respond(w, 200, map[string]any{"task_id": id, "status": t.Status, "chunks": chunks, "next_after": after, "retention_days": 7})
			return
		}
		if cancel && r.Method == "POST" {
			if !hasScope(c.Scope, "computer:control") {
				fail(w, 403, "control scope required")
				return
			}
			_, err = s.db.Exec("UPDATE tasks SET status=CASE WHEN status='queued' THEN 'cancelled' ELSE 'cancelling' END WHERE id=? AND status IN ('queued','dispatched','running','approval_pending')", id)
			if err != nil {
				fail(w, 500, "cancel failed")
				return
			}
			s.send(t.DeviceID, map[string]any{"type": "cancel", "task_id": id})
			_ = s.audit(c.Subject, t.DeviceID, id, "cancel_requested")
			t, _ = s.task(c.Subject, id)
		} else if r.Method != "GET" {
			fail(w, 405, "method not allowed")
			return
		}
		respond(w, 200, t)
	default:
		fail(w, 404, "route not found")
	}
}
func (s *Server) createTask(w http.ResponseWriter, r *http.Request, c *Claims) {
	var a struct {
		DeviceID string          `json:"device_id"`
		Action   string          `json:"action"`
		Args     json.RawMessage `json:"args"`
		Key      string          `json:"idempotency_key"`
		Timeout  int64           `json:"timeout_seconds"`
	}
	if !decode(w, r, &a) {
		return
	}
	cat, err := category(a.Action, a.Args)
	if err != nil || len(a.Key) < 8 || len(a.Key) > 128 || a.Timeout < 1 || a.Timeout > 600 || len(a.Args) == 0 || a.Args[0] != '{' {
		fail(w, 400, "invalid task")
		return
	}
	if cat != "READ" && !hasScope(c.Scope, "computer:control") {
		fail(w, 403, "control scope required")
		return
	}
	if !s.owned(c.Subject, a.DeviceID) {
		fail(w, 404, "device not found")
		return
	}
	if a.Action == "terminal_input" || a.Action == "terminal_screen" {
		var input struct {
			Target string   `json:"target_task_id"`
			Text   string   `json:"text"`
			EOF    bool     `json:"eof"`
			Keys   []string `json:"keys"`
			Cols   *int     `json:"cols"`
			Rows   *int     `json:"rows"`
		}
		if json.Unmarshal(a.Args, &input) != nil || len(input.Target) < 8 || len(input.Target) > 128 || len(input.Text) > 16384 || len(input.Keys) > 64 || ((input.Cols == nil) != (input.Rows == nil)) || (input.Cols != nil && (*input.Cols < 20 || *input.Cols > 240 || *input.Rows < 5 || *input.Rows > 100)) || (a.Action == "terminal_input" && input.Text == "" && !input.EOF && len(input.Keys) == 0 && input.Cols == nil) {
			fail(w, 400, "invalid stdin request")
			return
		}
		for _, key := range input.Keys {
			if len(key) > 16 {
				fail(w, 400, "invalid terminal key")
				return
			}
		}
		target, err := s.task(c.Subject, input.Target)
		if err != nil || target.DeviceID != a.DeviceID || target.Action != "terminal_execute" {
			fail(w, 404, "owned terminal task not found")
			return
		}
	}
	fingerprint := digest(a.Action + string(a.Args) + fmt.Sprint(a.Timeout))
	id := token(18)
	tx, err := s.db.Begin()
	if err != nil {
		fail(w, 500, "storage failed")
		return
	}
	defer tx.Rollback()
	res, err := tx.Exec(`INSERT INTO tasks(id,owner,device_id,action,args,idempotency_key,fingerprint,status,category,created_at,deadline) VALUES(?,?,?,?,?,?,?,'queued',?,?,?) ON CONFLICT(owner,device_id,idempotency_key) DO NOTHING`, id, c.Subject, a.DeviceID, a.Action, string(a.Args), a.Key, fingerprint, cat, time.Now().Unix(), time.Now().Unix()+a.Timeout)
	if err != nil {
		fail(w, 500, "task storage failed")
		return
	}
	n, _ := res.RowsAffected()
	if n == 0 {
		var oldFingerprint string
		err = tx.QueryRow("SELECT id,fingerprint FROM tasks WHERE owner=? AND device_id=? AND idempotency_key=?", c.Subject, a.DeviceID, a.Key).Scan(&id, &oldFingerprint)
		if err != nil || fingerprint != oldFingerprint {
			fail(w, 409, "idempotency key reused with different request")
			return
		}
	}
	if n == 1 {
		_, err = tx.Exec("INSERT INTO audit(at,owner,device_id,task_id,event) VALUES(?,?,?,?,?)", time.Now().Unix(), c.Subject, a.DeviceID, id, "queued")
		if err != nil {
			fail(w, 500, "audit failed")
			return
		}
	}
	if tx.Commit() != nil {
		fail(w, 500, "commit failed")
		return
	}
	t, err := s.task(c.Subject, id)
	if err != nil {
		fail(w, 500, "task lookup failed")
		return
	}
	respond(w, 202, t)
}
func (s *Server) send(id string, v any) bool {
	s.mu.Lock()
	sess := s.sessions[id]
	s.mu.Unlock()
	if sess == nil {
		return false
	}
	select {
	case sess.out <- v:
		return true
	default:
		sess.conn.Close()
		return false
	}
}

func (s *Server) connectAgent(w http.ResponseWriter, r *http.Request) {
	ip, _, _ := net.SplitHostPort(r.RemoteAddr)
	if !s.allowed("connect:"+ip, 120) {
		fail(w, 429, "connection rate limit")
		return
	}
	var proof struct {
		DeviceID   string `json:"device_id"`
		Timestamp  int64  `json:"timestamp"`
		Nonce      string `json:"nonce"`
		Signature  string `json:"signature"`
		JournalACK bool   `json:"journal_ack"`
	}
	data, err := base64.StdEncoding.DecodeString(r.Header.Get("X-Device-Proof"))
	if err != nil || len(data) > 2048 || json.Unmarshal(data, &proof) != nil {
		fail(w, 401, "invalid proof")
		return
	}
	var key, owner string
	err = s.db.QueryRow("SELECT public_key,owner FROM devices WHERE id=? AND revoked=0", proof.DeviceID).Scan(&key, &owner)
	pub, _ := base64.StdEncoding.DecodeString(key)
	sig, _ := base64.StdEncoding.DecodeString(proof.Signature)
	now := time.Now().Unix()
	if err != nil || len(pub) != 32 || len(proof.Nonce) < 20 || proof.Timestamp < now-30 || proof.Timestamp > now+5 || !ed25519.Verify(pub, []byte(fmt.Sprintf("%s\n%d\n%s", proof.DeviceID, proof.Timestamp, proof.Nonce)), sig) {
		fail(w, 401, "invalid device proof")
		return
	}
	_, _ = s.db.Exec("DELETE FROM nonces WHERE expires<?", now)
	if _, err = s.db.Exec("INSERT INTO nonces(nonce,expires) VALUES(?,?)", proof.Nonce, now+60); err != nil {
		fail(w, 401, "proof replay rejected")
		return
	}
	upgrader := websocket.Upgrader{CheckOrigin: func(r *http.Request) bool { return r.Header.Get("Origin") == "" }}
	conn, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		return
	}
	sess := &Session{conn: conn, out: make(chan any, 32), done: make(chan struct{})}
	s.mu.Lock()
	if old := s.sessions[proof.DeviceID]; old != nil {
		old.conn.Close()
	}
	s.sessions[proof.DeviceID] = sess
	s.mu.Unlock()
	defer func() {
		close(sess.done)
		conn.Close()
		s.mu.Lock()
		current := s.sessions[proof.DeviceID] == sess
		if current {
			delete(s.sessions, proof.DeviceID)
		}
		s.mu.Unlock()
		if current {
			_, _ = s.db.Exec("UPDATE tasks SET status='uncertain',error='connection lost before outcome acknowledgement' WHERE device_id=? AND status IN ('dispatched','running','approval_pending','cancelling')", proof.DeviceID)
		}
	}()
	conn.SetReadLimit(2 * 1024 * 1024)
	_ = conn.SetReadDeadline(time.Now().Add(45 * time.Second))
	go func() {
		for {
			select {
			case <-sess.done:
				return
			case v := <-sess.out:
				conn.SetWriteDeadline(time.Now().Add(10 * time.Second))
				if conn.WriteJSON(v) != nil {
					conn.Close()
					return
				}
			}
		}
	}()
	s.send(proof.DeviceID, map[string]any{"type": "hello", "protocol_version": 1, "streaming_output": true})
	for {
		var msg struct {
			Type   string          `json:"type"`
			TaskID string          `json:"task_id"`
			Status string          `json:"status"`
			Result json.RawMessage `json:"result"`
			Error  string          `json:"error"`
			Seq    int64           `json:"seq"`
			Stream string          `json:"stream"`
			Text   string          `json:"text"`
		}
		if conn.ReadJSON(&msg) != nil {
			return
		}
		conn.SetReadDeadline(time.Now().Add(45 * time.Second))
		_, _ = s.db.Exec("UPDATE devices SET last_seen=? WHERE id=?", time.Now().Unix(), proof.DeviceID)
		if msg.Type == "heartbeat" {
			s.send(proof.DeviceID, map[string]string{"type": "heartbeat"})
			continue
		}
		if msg.Type == "output" && proof.JournalACK {
			if msg.Seq < 1 || msg.Seq > 512 || len(msg.Text) > 16384 || (msg.Stream != "stdout" && msg.Stream != "stderr") {
				return
			}
			t, err := s.task(owner, msg.TaskID)
			if err != nil || t.DeviceID != proof.DeviceID || t.Action != "terminal_execute" {
				return
			}
			_, err = s.db.Exec("INSERT INTO task_output(task_id,seq,stream,text,at) VALUES(?,?,?,?,?) ON CONFLICT(task_id,seq) DO NOTHING", msg.TaskID, msg.Seq, msg.Stream, msg.Text, time.Now().Unix())
			if err != nil {
				return
			}
			var stream, text string
			if s.db.QueryRow("SELECT stream,text FROM task_output WHERE task_id=? AND seq=?", msg.TaskID, msg.Seq).Scan(&stream, &text) != nil || stream != msg.Stream || text != msg.Text {
				return
			}
			s.send(proof.DeviceID, map[string]any{"type": "output_ack", "task_id": msg.TaskID, "seq": msg.Seq})
			continue
		}
		if msg.Type != "result" {
			return
		}
		switch msg.Status {
		case "running", "approval_pending", "succeeded", "failed", "cancelled", "uncertain":
		default:
			return
		}
		if len(msg.Error) > 1024 || len(msg.Result) > 1024*1024 {
			return
		}
		if len(msg.Result) == 0 {
			msg.Result = json.RawMessage("null")
		}
		completed := any(nil)
		if msg.Status != "running" && msg.Status != "approval_pending" {
			completed = time.Now().Unix()
		}
		tx, err := s.db.Begin()
		if err != nil {
			return
		}
		res, err := tx.Exec("UPDATE tasks SET status=?,started_at=COALESCE(started_at,?),completed_at=?,result=?,error=? WHERE id=? AND device_id=? AND owner=? AND status IN ('dispatched','running','approval_pending','cancelling','uncertain')", msg.Status, time.Now().Unix(), completed, string(msg.Result), msg.Error, msg.TaskID, proof.DeviceID, owner)
		if err != nil {
			tx.Rollback()
			return
		}
		n, _ := res.RowsAffected()
		if n > 0 {
			if _, err = tx.Exec("INSERT INTO audit(at,owner,device_id,task_id,event) VALUES(?,?,?,?,?)", time.Now().Unix(), owner, proof.DeviceID, msg.TaskID, msg.Status); err != nil {
				tx.Rollback()
				return
			}
		}
		if tx.Commit() != nil {
			return
		}
		if completed != nil && proof.JournalACK {
			// A duplicate after a lost ACK is acknowledged only if its terminal
			// status agrees with the durable, owned task. Never ACK before commit.
			var stored string
			if s.db.QueryRow("SELECT status FROM tasks WHERE id=? AND device_id=? AND owner=?", msg.TaskID, proof.DeviceID, owner).Scan(&stored) == nil && stored == msg.Status {
				s.send(proof.DeviceID, map[string]any{"type": "ack", "task_id": msg.TaskID, "status": stored})
			}
		}
	}
}
func (s *Server) dispatch() {
	ticker := time.NewTicker(250 * time.Millisecond)
	defer ticker.Stop()
	nextPrune := int64(0)
	for {
		select {
		case <-s.ctx.Done():
			return
		case <-ticker.C:
			if now := time.Now().Unix(); now >= nextPrune {
				if err := s.pruneOutput(now); err != nil {
					slog.Error("terminal output retention failed", "error", err)
				}
				nextPrune = now + 60*60
			}
			// Allow executors three seconds to commit timeout/cancel results, then report unknown outcomes.
			_, _ = s.db.Exec("UPDATE tasks SET status='uncertain',error='execution deadline passed without a committed outcome' WHERE status IN ('dispatched','running','approval_pending','cancelling') AND deadline<?", time.Now().Unix()-3)
			_, _ = s.db.Exec("UPDATE tasks SET status='expired',completed_at=?,error='deadline passed before dispatch' WHERE status='queued' AND deadline<=?", time.Now().Unix(), time.Now().Unix())
			rows, err := s.db.Query("SELECT id,owner,device_id FROM tasks WHERE status='queued' LIMIT 64")
			if err != nil {
				continue
			}
			type queued struct{ id, owner, device string }
			tasks := []queued{}
			for rows.Next() {
				var q queued
				if rows.Scan(&q.id, &q.owner, &q.device) == nil {
					tasks = append(tasks, q)
				}
			}
			rows.Close()
			for _, q := range tasks {
				s.mu.Lock()
				_, online := s.sessions[q.device]
				s.mu.Unlock()
				if !online {
					continue
				}
				res, err := s.db.Exec("UPDATE tasks SET status='dispatched' WHERE id=? AND status='queued'", q.id)
				if err != nil {
					continue
				}
				n, _ := res.RowsAffected()
				if n == 0 {
					continue
				}
				t, err := s.task(q.owner, q.id)
				if err != nil {
					continue
				}
				payload, _ := json.Marshal(t)
				sig := ed25519.Sign(s.signer, payload)
				if !s.send(q.device, map[string]any{"type": "task", "payload": base64.StdEncoding.EncodeToString(payload), "signature": base64.StdEncoding.EncodeToString(sig)}) {
					_, _ = s.db.Exec("UPDATE tasks SET status='uncertain',error='dispatch delivery uncertain' WHERE id=? AND status='dispatched'", q.id)
				}
			}
		}
	}
}
func (s *Server) close() {
	s.cancel()
	s.mu.Lock()
	for _, v := range s.sessions {
		v.conn.Close()
	}
	s.mu.Unlock()
}
func secretEnv(name string) string {
	if path := os.Getenv(name + "_FILE"); path != "" {
		data, err := os.ReadFile(path)
		if err != nil {
			slog.Error("cannot read service secret file", "variable", name)
			os.Exit(1)
		}
		return strings.TrimSpace(string(data))
	}
	return os.Getenv(name)
}
func main() {
	cfg := Config{Addr: os.Getenv("GATEWAY_ADDR"), PublicAddr: os.Getenv("GATEWAY_PUBLIC_ADDR"), Database: os.Getenv("GATEWAY_DB"), BridgeKey: secretEnv("BRIDGE_KEY"), SigningSeed: secretEnv("GATEWAY_SIGNING_SEED"), Dev: os.Getenv("OPENCOMMANDER_DEV") == "1"}
	if cfg.Addr == "" {
		cfg.Addr = "127.0.0.1:8080"
	}
	if cfg.Database == "" {
		cfg.Database = "gateway.db"
	}
	s, err := newServer(cfg)
	if err != nil {
		slog.Error("startup failed", "error", err)
		os.Exit(1)
	}
	defer s.db.Close()
	httpServer := &http.Server{Addr: cfg.Addr, Handler: s.handler(), ReadHeaderTimeout: 10 * time.Second, IdleTimeout: 60 * time.Second, MaxHeaderBytes: 8192}
	servers := []*http.Server{httpServer}
	if cfg.PublicAddr != "" {
		if cfg.PublicAddr == cfg.Addr {
			slog.Error("public and private listeners must have different addresses")
			os.Exit(1)
		}
		servers = append(servers, &http.Server{Addr: cfg.PublicAddr, Handler: s.publicHandler(), ReadHeaderTimeout: 10 * time.Second, IdleTimeout: 60 * time.Second, MaxHeaderBytes: 8192})
	}
	go s.dispatch()
	for _, listener := range servers {
		go func() {
			slog.Info("gateway listening", "address", listener.Addr)
			if err := listener.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
				slog.Error("http server stopped", "error", err)
				s.cancel()
			}
		}()
	}
	ctx, stop := signal.NotifyContext(s.ctx, os.Interrupt, syscall.SIGTERM)
	defer stop()
	<-ctx.Done()
	s.close()
	shutdown, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	for _, listener := range servers {
		_ = listener.Shutdown(shutdown)
	}
}
