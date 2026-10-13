package tasks

import (
	"fmt"
	"sync"
	"time"
)

// Task is a persistent unit of work routed to one device.
type Task struct {
	ID            string         `json:"task_id"`
	DeviceID      string         `json:"device_id"`
	OwnerID       string         `json:"owner_id"`
	Op            string         `json:"op"`
	Params        map[string]any `json:"params"`
	State         string         `json:"state"`
	Result        map[string]any `json:"result,omitempty"`
	Error         string         `json:"error,omitempty"`
	Idempotency   string         `json:"idempotency_key,omitempty"`
	CreatedAt     time.Time      `json:"created_at"`
	DeadlineMs    int            `json:"deadline_ms"`
}

type Device struct {
	ID       string    `json:"device_id"`
	OwnerID  string    `json:"owner_id"`
	Name     string    `json:"name"`
	Online   bool      `json:"online"`
	LastSeen time.Time `json:"last_seen"`
	Paused   bool      `json:"paused"`
}

type Store struct {
	mu      sync.Mutex
	tasks   map[string]*Task
	byIdem  map[string]string
	devices map[string]*Device
	codes   map[string]string // pairing code -> owner
	n       int
}

func NewStore() *Store {
	return &Store{tasks: map[string]*Task{}, byIdem: map[string]string{}, devices: map[string]*Device{}, codes: map[string]string{}}
}

func (s *Store) CreateTask(owner, device, op string, params map[string]any, idem string) *Task {
	s.mu.Lock()
	defer s.mu.Unlock()
	if idem != "" {
		if id, ok := s.byIdem[idem]; ok {
			return s.tasks[id]
		}
	}
	s.n++
	id := fmt.Sprintf("task_%d_%d", time.Now().Unix(), s.n)
	t := &Task{ID: id, DeviceID: device, OwnerID: owner, Op: op, Params: params, State: "queued", Idempotency: idem, CreatedAt: time.Now(), DeadlineMs: 30000}
	s.tasks[id] = t
	if idem != "" {
		s.byIdem[idem] = id
	}
	return t
}

func (s *Store) GetTask(id string) (*Task, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	t, ok := s.tasks[id]
	return t, ok
}

func (s *Store) SetResult(id string, ok bool, result map[string]any, errMsg string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	t, found := s.tasks[id]
	if !found {
		return
	}
	if ok {
		t.State = "succeeded"
		t.Result = result
	} else {
		t.State = "failed"
		t.Error = errMsg
	}
}

func (s *Store) Cancel(id string) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	t, ok := s.tasks[id]
	if !ok {
		return false
	}
	if t.State == "succeeded" || t.State == "failed" || t.State == "cancelled" {
		return false
	}
	t.State = "cancelled"
	return true
}

func (s *Store) Heartbeat(deviceID, owner string, paused bool) *Device {
	s.mu.Lock()
	defer s.mu.Unlock()
	d, ok := s.devices[deviceID]
	if !ok {
		d = &Device{ID: deviceID, OwnerID: owner, Name: deviceID}
		s.devices[deviceID] = d
	}
	d.Online = true
	d.LastSeen = time.Now()
	d.Paused = paused
	return d
}

func (s *Store) ListDevices(owner string) []*Device {
	s.mu.Lock()
	defer s.mu.Unlock()
	var out []*Device
	for _, d := range s.devices {
		if d.OwnerID == owner || owner == "" {
			if time.Since(d.LastSeen) > 90*time.Second {
				d.Online = false
			}
			out = append(out, d)
		}
	}
	return out
}

func (s *Store) DeviceStatus(id string) (*Device, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	d, ok := s.devices[id]
	if ok && time.Since(d.LastSeen) > 90*time.Second {
		d.Online = false
	}
	return d, ok
}

func (s *Store) NewPairingCode(owner string) string {
	s.mu.Lock()
	defer s.mu.Unlock()
	code := fmt.Sprintf("%06d", time.Now().UnixNano()%1000000)
	s.codes[code] = owner
	return code
}

func (s *Store) ClaimDevice(code, deviceID string) (string, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	owner, ok := s.codes[code]
	if !ok {
		return "", false
	}
	delete(s.codes, code)
	d, found := s.devices[deviceID]
	if !found {
		d = &Device{ID: deviceID, Name: deviceID}
		s.devices[deviceID] = d
	}
	d.OwnerID = owner
	return owner, true
}
