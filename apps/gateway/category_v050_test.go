package main

import (
	"encoding/json"
	"testing"
)

func TestV050TaskCategories(t *testing.T) {
	cases := []struct{ action, expected string }{
		{"file_export", "READ"},
		{"workflow", "SENSITIVE"},
		{"wordcraft_compose", "SENSITIVE"},
		{"visual_agent", "SENSITIVE"},
	}
	for _, tc := range cases {
		got, err := category(tc.action, json.RawMessage(`{}`))
		if err != nil || got != tc.expected {
			t.Errorf("category(%q) = %q, %v; want %q", tc.action, got, err, tc.expected)
		}
	}
}

func TestMangaMotionTaskCategories(t *testing.T) {
	cases := []struct{ op, expected string }{
		{"list", "READ"}, {"inspect", "READ"},
		{"patch", "SENSITIVE"},
		{"compile_motion", "SENSITIVE"},
		{"import_image", "SENSITIVE"},
		{"set_text", "SENSITIVE"}, {"render_still", "SENSITIVE"}, {"render_video", "SENSITIVE"},
	}
	for _, c := range cases {
		arg, _ := json.Marshal(map[string]string{"op": c.op})
		got, err := category("motiongraphics", arg)
		if err != nil || got != c.expected {
			t.Errorf("mangamotion %s: got %s, err=%v, expected %s", c.op, got, err, c.expected)
		}
	}
	for _, payload := range []string{`{"op":"arbitrary_shell"}`, `{`, `{}`} {
		if got, err := category("motiongraphics", json.RawMessage(payload)); err == nil {
			t.Errorf("unsupported task accepted: %q (%s)", payload, got)
		}
	}
}
