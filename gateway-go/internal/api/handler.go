package api

import (
	"encoding/json"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/gorilla/websocket"
	"opencommander/gateway/internal/tasks"
)

type Handler struct {
	store   *tasks.Store
	limiter map[string][]time.Time
	mu      sync.Mutex
	agentTx map[string]chan map[string]any
}

func New(store *tasks.Store) *Handler {
	return &Handler{store: store, limiter: map[string][]time.Time{}, agentTx: map[string]chan map[string]any{}}
}

func (h *Handler) owner(r *http.Request) string {
	return r.Header.Get("X-Owner-Id")
}

func (h *Handler) rateOk(owner string) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	now := time.Now()
	keep := h.limiter[owner][:0]
	for _, t := range h.limiter[owner] {
		if now.Sub(t) < time.Minute {
			keep = append(keep, t)
		}
	}
	if len(keep) >= 60 {
		h.limiter[owner] = keep
		return false
	}
	h.limiter[owner] = append(keep, now)
	return true
}

var upgrader = websocket.Upgrader{CheckOrigin: func(r *http.Request) bool { return true }}

// GET /health
func (h *Handler) Routes(mux *http.ServeMux) {
	mux.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) {
		json.NewEncoder(w).Encode(map[string]bool{"ok": true})
	})
	mux.HandleFunc("/agent/v1/connect", h.agentConnect)
	mux.HandleFunc("/internal/v1/devices", h.listDevices)
	mux.HandleFunc("/internal/v1/devices/", h.deviceSub)
	mux.HandleFunc("/internal/v1/tasks", h.createTask)
	mux.HandleFunc("/internal/v1/tasks/", h.taskSub)
	mux.HandleFunc("/internal/v1/pairing/code", h.pairingCode)
	mux.HandleFunc("/internal/v1/pairing/claim", h.pairingClaim)
}

func writeJSON(w http.ResponseWriter, code int, v any) {
	w.Header().Set("content-type", "application/json")
	w.WriteHeader(code)
	json.NewEncoder(w).Encode(v)
}

func (h *Handler) listDevices(w http.ResponseWriter, r *http.Request) {
	if !h.rateOk(h.owner(r)) {
		writeJSON(w, 429, map[string]string{"error": "rate limited"})
		return
	}
	writeJSON(w, 200, map[string]any{"devices": h.store.ListDevices(h.owner(r))})
}

func (h *Handler) deviceSub(w http.ResponseWriter, r *http.Request) {
	rest := strings.TrimPrefix(r.URL.Path, "/internal/v1/devices/")
	parts := strings.Split(rest, "/")
	if len(parts) == 2 && parts[1] == "status" {
		d, ok := h.store.DeviceStatus(parts[0])
		if !ok {
			writeJSON(w, 404, map[string]string{"error": "unknown device"})
			return
		}
		writeJSON(w, 200, d)
		return
	}
	writeJSON(w, 404, map[string]string{"error": "not found"})
}

func (h *Handler) createTask(w http.ResponseWriter, r *http.Request) {
	if r.Method != "POST" {
		writeJSON(w, 405, map[string]string{"error": "method not allowed"})
		return
	}
	owner := h.owner(r)
	if owner == "" {
		writeJSON(w, 401, map[string]string{"error": "missing X-Owner-Id"})
		return
	}
	if !h.rateOk(owner) {
		writeJSON(w, 429, map[string]string{"error": "rate limited"})
		return
	}
	var body struct {
		DeviceID string         `json:"device_id"`
		Op       string         `json:"op"`
		Params   map[string]any `json:"params"`
	}
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil || body.DeviceID == "" || body.Op == "" {
		writeJSON(w, 400, map[string]string{"error": "bad request"})
		return
	}
	d, ok := h.store.DeviceStatus(body.DeviceID)
	if !ok || (d.OwnerID != owner && d.OwnerID != "") {
		writeJSON(w, 403, map[string]string{"error": "not your device"})
		return
	}
	if d != nil && d.Paused {
		writeJSON(w, 423, map[string]string{"error": "device paused by owner (emergency pause)"})
		return
	}
	idem := r.Header.Get("X-Idempotency-Key")
	t := h.store.CreateTask(owner, body.DeviceID, body.Op, body.Params, idem)
	h.mu.Lock()
	ch, hasAgent := h.agentTx[body.DeviceID]
	h.mu.Unlock()
	if hasAgent {
		select {
		case ch <- map[string]any{"type": "task_assign", "task_id": t.ID, "op": t.Op, "params": t.Params, "deadline_ms": t.DeadlineMs, "idempotency_key": idem}:
			t.State = "dispatched"
		default:
		}
	}
	writeJSON(w, 200, t)
}

func (h *Handler) taskSub(w http.ResponseWriter, r *http.Request) {
	rest := strings.TrimPrefix(r.URL.Path, "/internal/v1/tasks/")
	if strings.HasSuffix(rest, "/cancel") {
		id := strings.TrimSuffix(rest, "/cancel")
		if h.store.Cancel(id) {
			writeJSON(w, 200, map[string]string{"status": "cancelled"})
			return
		}
		writeJSON(w, 409, map[string]string{"error": "cannot cancel"})
		return
	}
	t, ok := h.store.GetTask(rest)
	if !ok {
		writeJSON(w, 404, map[string]string{"error": "unknown task"})
		return
	}
	if t.OwnerID != h.owner(r) && h.owner(r) != "" {
		writeJSON(w, 403, map[string]string{"error": "not your task"})
		return
	}
	writeJSON(w, 200, t)
}

func (h *Handler) pairingCode(w http.ResponseWriter, r *http.Request) {
	owner := h.owner(r)
	if owner == "" {
		writeJSON(w, 401, map[string]string{"error": "missing owner"})
		return
	}
	writeJSON(w, 200, map[string]string{"code": h.store.NewPairingCode(owner)})
}

func (h *Handler) pairingClaim(w http.ResponseWriter, r *http.Request) {
	var body struct {
		Code     string `json:"code"`
		DeviceID string `json:"device_id"`
	}
	json.NewDecoder(r.Body).Decode(&body)
	owner, ok := h.store.ClaimDevice(body.Code, body.DeviceID)
	if !ok {
		writeJSON(w, 400, map[string]string{"error": "bad code"})
		return
	}
	writeJSON(w, 200, map[string]string{"owner_id": owner})
}

func (h *Handler) agentConnect(w http.ResponseWriter, r *http.Request) {
	deviceID := r.URL.Query().Get("device_id")
	if deviceID == "" {
		writeJSON(w, 400, map[string]string{"error": "missing device_id"})
		return
	}
	// NOTE: device token validation against credential store goes here (MVP: accept + bind on pairing claim).
	c, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		return
	}
	defer c.Close()
	tx := make(chan map[string]any, 16)
	h.mu.Lock()
	h.agentTx[deviceID] = tx
	h.mu.Unlock()
	defer func() {
		h.mu.Lock()
		delete(h.agentTx, deviceID)
		h.mu.Unlock()
	}()
	h.store.Heartbeat(deviceID, "", false)
	go func() {
		for msg := range tx {
			_ = c.WriteJSON(msg)
		}
	}()
	c.SetReadDeadline(time.Now().Add(120 * time.Second))
	for {
		var frame map[string]any
		if err := c.ReadJSON(&frame); err != nil {
			return
		}
		typ, _ := frame["type"].(string)
		switch typ {
		case "heartbeat":
			paused, _ := frame["paused"].(bool)
			d, _ := h.store.DeviceStatus(deviceID)
			owner := ""
			if d != nil {
				owner = d.OwnerID
			}
			h.store.Heartbeat(deviceID, owner, paused)
		case "task_ack", "task_result":
			id, _ := frame["task_id"].(string)
			if typ == "task_result" {
				ok, _ := frame["ok"].(bool)
				res, _ := frame["result"].(map[string]any)
				errMsg, _ := frame["error"].(string)
				h.store.SetResult(id, ok, res, errMsg)
			}
		}
		c.SetReadDeadline(time.Now().Add(120 * time.Second))
	}
}
