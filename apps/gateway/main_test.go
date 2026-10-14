package main

import (
	"bytes"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/gorilla/websocket"
)

func fixture(t *testing.T) (*Server, *httptest.Server) {
	t.Helper()
	s, err := newServer(Config{Database: filepath.Join(t.TempDir(), "gateway.db"), BridgeKey: strings.Repeat("b", 32), SigningSeed: base64.StdEncoding.EncodeToString(bytes.Repeat([]byte{7}, 32)), Dev: true})
	if err != nil {
		t.Fatal(err)
	}
	h := httptest.NewServer(s.handler())
	t.Cleanup(func() { s.close(); h.Close(); s.db.Close() })
	return s, h
}
func TestPublicListenerCannotReachInternalAPI(t *testing.T) {
	s, _ := fixture(t)
	h := httptest.NewServer(s.publicHandler())
	defer h.Close()
	for _, path := range []string{"/api/devices", "/api/pair/claim", "/api/../api/devices", "/api%2fdevices"} {
		r, err := http.NewRequest(http.MethodGet, h.URL+path, nil)
		if err != nil {
			t.Fatal(err)
		}
		r.Header.Set("Authorization", "Bearer "+identity(t, s, "alice", "computer:read computer:control"))
		res, err := http.DefaultClient.Do(r)
		if err != nil {
			t.Fatal(err)
		}
		res.Body.Close()
		if res.StatusCode != http.StatusNotFound {
			t.Fatalf("public route %s: %d", path, res.StatusCode)
		}
	}
	if code, _ := request(t, s, h, "GET", "/healthz", "", nil); code != 200 {
		t.Fatal(code)
	}
	if code, _ := request(t, s, h, "POST", "/v1/enroll", "", map[string]string{"name": "railway-test", "public_key": base64.StdEncoding.EncodeToString(bytes.Repeat([]byte{3}, 32))}); code != 201 {
		t.Fatal(code)
	}
}
func identity(t *testing.T, s *Server, owner, scope string) string {
	t.Helper()
	v, err := jwt.NewWithClaims(jwt.SigningMethodHS256, Claims{RegisteredClaims: jwt.RegisteredClaims{Subject: owner, Issuer: "opencommander-bridge", Audience: []string{"opencommander-gateway"}, IssuedAt: jwt.NewNumericDate(time.Now()), ExpiresAt: jwt.NewNumericDate(time.Now().Add(15 * time.Second))}, Scope: scope, TokenID: "test-token"}).SignedString([]byte(s.cfg.BridgeKey))
	if err != nil {
		t.Fatal(err)
	}
	return v
}
func request(t *testing.T, s *Server, h *httptest.Server, method, path, owner string, body any) (int, map[string]any) {
	t.Helper()
	b, _ := json.Marshal(body)
	r, _ := http.NewRequest(method, h.URL+path, bytes.NewReader(b))
	if owner != "" {
		r.Header.Set("Authorization", "Bearer "+identity(t, s, owner, "computer:read computer:control"))
	}
	res, err := http.DefaultClient.Do(r)
	if err != nil {
		t.Fatal(err)
	}
	defer res.Body.Close()
	var v map[string]any
	if err = json.NewDecoder(res.Body).Decode(&v); err != nil {
		t.Fatal(err)
	}
	return res.StatusCode, v
}
func pair(t *testing.T, s *Server, h *httptest.Server) (string, ed25519.PrivateKey) {
	t.Helper()
	key := ed25519.NewKeyFromSeed(bytes.Repeat([]byte{3}, 32))
	code, e := request(t, s, h, "POST", "/v1/enroll", "", map[string]string{"name": "test", "public_key": base64.StdEncoding.EncodeToString(key.Public().(ed25519.PublicKey))})
	if code != 201 {
		t.Fatal(code, e)
	}
	code, p := request(t, s, h, "POST", "/api/pair/claim", "alice", map[string]any{"code": e["pairing_code"]})
	if code != 200 {
		t.Fatal(code, p)
	}
	return p["device_id"].(string), key
}
func taskBody(device, key string) map[string]any {
	return map[string]any{"device_id": device, "action": "system_info", "args": map[string]any{}, "idempotency_key": key, "timeout_seconds": 60}
}
func TestAuthenticationAndOwnership(t *testing.T) {
	s, h := fixture(t)
	device, _ := pair(t, s, h)
	if code, _ := request(t, s, h, "GET", "/api/devices", "", nil); code != 401 {
		t.Fatal(code)
	}
	if code, _ := request(t, s, h, "POST", "/api/tasks", "mallory", taskBody(device, "request-1")); code != 404 {
		t.Fatal(code)
	}
	_, v := request(t, s, h, "GET", "/api/devices", "mallory", nil)
	if len(v["devices"].([]any)) != 0 {
		t.Fatal(v)
	}
	r, _ := http.NewRequest("GET", h.URL+"/api/devices", nil)
	r.Header.Set("Authorization", "Bearer invalid")
	res, _ := http.DefaultClient.Do(r)
	res.Body.Close()
	if res.StatusCode != 401 {
		t.Fatal(res.StatusCode)
	}
}
func TestPairingExpiryAndOneUse(t *testing.T) {
	s, h := fixture(t)
	device, _ := pair(t, s, h)
	var hash string
	s.db.QueryRow("SELECT code_hash FROM enrollments WHERE id=?", device).Scan(&hash)
	if code, _ := request(t, s, h, "POST", "/api/pair/claim", "alice", map[string]string{"code": "not-the-code"}); code != 403 {
		t.Fatal(code)
	}
	key := base64.StdEncoding.EncodeToString(bytes.Repeat([]byte{2}, 32))
	_, e := request(t, s, h, "POST", "/v1/enroll", "", map[string]string{"name": "expired", "public_key": key})
	s.db.Exec("UPDATE enrollments SET expires=0 WHERE id=?", e["enrollment_id"])
	if code, _ := request(t, s, h, "POST", "/api/pair/claim", "alice", map[string]any{"code": e["pairing_code"]}); code != 403 {
		t.Fatal(code)
	}
	if code, _ := request(t, s, h, "POST", "/v1/enroll/"+device+"/poll", "", map[string]string{"secret": "wrong"}); code != 403 {
		t.Fatal(code)
	}
	_, fresh := request(t, s, h, "POST", "/v1/enroll", "", map[string]string{"name": "one-use", "public_key": key})
	codeBody := map[string]any{"code": fresh["pairing_code"]}
	if code, _ := request(t, s, h, "POST", "/api/pair/claim", "alice", codeBody); code != 200 {
		t.Fatal(code)
	}
	if code, _ := request(t, s, h, "POST", "/api/pair/claim", "mallory", codeBody); code != 403 {
		t.Fatal("pairing code reused", code)
	}
}
func TestIdempotencyAndConflict(t *testing.T) {
	s, h := fixture(t)
	device, _ := pair(t, s, h)
	code, a := request(t, s, h, "POST", "/api/tasks", "alice", taskBody(device, "request-1"))
	if code != 202 {
		t.Fatal(code, a)
	}
	_, b := request(t, s, h, "POST", "/api/tasks", "alice", taskBody(device, "request-1"))
	if a["task_id"] != b["task_id"] {
		t.Fatal(a, b)
	}
	other := taskBody(device, "request-1")
	other["action"] = "desktop_windows"
	if code, _ := request(t, s, h, "POST", "/api/tasks", "alice", other); code != 409 {
		t.Fatal(code)
	}
}
func TestQueuedCancellation(t *testing.T) {
	s, h := fixture(t)
	device, _ := pair(t, s, h)
	_, a := request(t, s, h, "POST", "/api/tasks", "alice", taskBody(device, "request-1"))
	if code, _ := request(t, s, h, "POST", "/api/tasks/"+a["task_id"].(string)+"/cancel", "mallory", map[string]any{}); code != 404 {
		t.Fatal(code)
	}
	_, b := request(t, s, h, "POST", "/api/tasks/"+a["task_id"].(string)+"/cancel", "alice", map[string]any{})
	if b["status"] != "cancelled" {
		t.Fatal(b)
	}
}
func TestRestartDoesNotReplay(t *testing.T) {
	s, h := fixture(t)
	device, _ := pair(t, s, h)
	_, a := request(t, s, h, "POST", "/api/tasks", "alice", taskBody(device, "request-1"))
	s.db.Exec("UPDATE tasks SET status='running' WHERE id=?", a["task_id"])
	db, err := openStore(s.cfg.Database)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var status string
	db.QueryRow("SELECT status FROM tasks WHERE id=?", a["task_id"]).Scan(&status)
	if status != "uncertain" {
		t.Fatal(status)
	}
}
func TestActiveDeadlineBecomesUncertainWithoutReplay(t *testing.T) {
	s, h := fixture(t)
	device, _ := pair(t, s, h)
	_, a := request(t, s, h, "POST", "/api/tasks", "alice", taskBody(device, "deadline-test"))
	if _, err := s.db.Exec("UPDATE tasks SET status='running',deadline=? WHERE id=?", time.Now().Unix()-10, a["task_id"]); err != nil {
		t.Fatal(err)
	}
	go s.dispatch()
	for i := 0; i < 30; i++ {
		task, err := s.task("alice", a["task_id"].(string))
		if err != nil {
			t.Fatal(err)
		}
		if task.Status == "uncertain" {
			return
		}
		time.Sleep(20 * time.Millisecond)
	}
	t.Fatal("active task deadline was not reconciled")
}
func TestRateLimit(t *testing.T) {
	s, _ := fixture(t)
	for i := 0; i < 5; i++ {
		if !s.allowed("test", 5) {
			t.Fatal(i)
		}
	}
	if s.allowed("test", 5) {
		t.Fatal("limit bypass")
	}
}
func TestExpiredAndWrongAudienceAssertions(t *testing.T) {
	s, h := fixture(t)
	for _, audience := range []string{"wrong", "opencommander-gateway"} {
		claims := Claims{RegisteredClaims: jwt.RegisteredClaims{Subject: "alice", Issuer: "opencommander-bridge", Audience: []string{audience}, IssuedAt: jwt.NewNumericDate(time.Now().Add(-time.Minute)), ExpiresAt: jwt.NewNumericDate(time.Now().Add(-time.Second))}, Scope: "computer:read", TokenID: "test"}
		raw, err := jwt.NewWithClaims(jwt.SigningMethodHS256, claims).SignedString([]byte(s.cfg.BridgeKey))
		if err != nil {
			t.Fatal(err)
		}
		r, _ := http.NewRequest("GET", h.URL+"/api/devices", nil)
		r.Header.Set("Authorization", "Bearer "+raw)
		res, err := http.DefaultClient.Do(r)
		if err != nil {
			t.Fatal(err)
		}
		res.Body.Close()
		if res.StatusCode != 401 {
			t.Fatal(res.StatusCode)
		}
	}
}
func TestStrictBodies(t *testing.T) {
	s, h := fixture(t)
	device, _ := pair(t, s, h)
	b := taskBody(device, "request-1")
	b["owner"] = "alice"
	if code, _ := request(t, s, h, "POST", "/api/tasks", "alice", b); code != 400 {
		t.Fatal(code)
	}
	if code, _ := request(t, s, h, "POST", "/api/tasks", "alice", map[string]any{"device_id": device, "action": "unknown"}); code != 400 {
		t.Fatal(code)
	}
}
func TestRoutingSignatureHeartbeatAndProofReplay(t *testing.T) {
	s, h := fixture(t)
	device, key := pair(t, s, h)
	timestamp := time.Now().Unix()
	nonce := token(24)
	message := []byte(device + "\n" + jsonNumber(timestamp) + "\n" + nonce)
	proof, _ := json.Marshal(map[string]any{"device_id": device, "timestamp": timestamp, "nonce": nonce, "signature": base64.StdEncoding.EncodeToString(ed25519.Sign(key, message)), "journal_ack": true})
	headers := http.Header{"X-Device-Proof": []string{base64.StdEncoding.EncodeToString(proof)}}
	conn, _, err := websocket.DefaultDialer.Dial("ws"+strings.TrimPrefix(h.URL, "http")+"/v1/agent", headers)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	var hello map[string]any
	conn.ReadJSON(&hello)
	if hello["type"] != "hello" {
		t.Fatal(hello)
	}
	replay, res, err := websocket.DefaultDialer.Dial("ws"+strings.TrimPrefix(h.URL, "http")+"/v1/agent", headers)
	if replay != nil {
		replay.Close()
	}
	if err == nil || res.StatusCode != 401 {
		t.Fatal("proof replay accepted")
	}
	res.Body.Close()
	go s.dispatch()
	_, a := request(t, s, h, "POST", "/api/tasks", "alice", taskBody(device, "request-1"))
	conn.SetReadDeadline(time.Now().Add(3 * time.Second))
	var task map[string]any
	if err = conn.ReadJSON(&task); err != nil {
		t.Fatal(err)
	}
	payload, _ := base64.StdEncoding.DecodeString(task["payload"].(string))
	sig, _ := base64.StdEncoding.DecodeString(task["signature"].(string))
	if !ed25519.Verify(s.signer.Public().(ed25519.PublicKey), payload, sig) {
		t.Fatal("invalid task signature")
	}
	conn.WriteJSON(map[string]any{"type": "result", "task_id": a["task_id"], "status": "succeeded", "result": map[string]string{"source": "test-protocol-simulator"}})
	var ack map[string]any
	if err = conn.ReadJSON(&ack); err != nil || ack["type"] != "ack" || ack["task_id"] != a["task_id"] || ack["status"] != "succeeded" {
		t.Fatal("missing durable acknowledgement", ack, err)
	}
	if stored, err := s.task("alice", a["task_id"].(string)); err != nil || stored.Status != "succeeded" {
		t.Fatal("ACK preceded durable result", err)
	}
	conn.WriteJSON(map[string]any{"type": "result", "task_id": a["task_id"], "status": "succeeded", "result": map[string]string{"source": "duplicate-must-not-overwrite"}})
	if err = conn.ReadJSON(&ack); err != nil || ack["type"] != "ack" {
		t.Fatal("lost ACK reconciliation failed", ack, err)
	}
	if stored, _ := s.task("alice", a["task_id"].(string)); strings.Contains(string(stored.Result), "duplicate") {
		t.Fatal("terminal result overwritten")
	}
	for i := 0; i < 20; i++ {
		_, v := request(t, s, h, "GET", "/api/tasks/"+a["task_id"].(string), "alice", nil)
		if v["status"] == "succeeded" {
			return
		}
		time.Sleep(20 * time.Millisecond)
	}
	t.Fatal("result not routed")
}
func jsonNumber(n int64) string { b, _ := json.Marshal(n); return string(b) }
func TestRevocation(t *testing.T) {
	s, h := fixture(t)
	device, _ := pair(t, s, h)
	if code, _ := request(t, s, h, "DELETE", "/api/devices/"+device, "mallory", nil); code != 404 {
		t.Fatal(code)
	}
	if code, _ := request(t, s, h, "DELETE", "/api/devices/"+device, "alice", nil); code != 200 {
		t.Fatal(code)
	}
	if s.owned("alice", device) {
		t.Fatal("revoked device remains authorized")
	}
}
