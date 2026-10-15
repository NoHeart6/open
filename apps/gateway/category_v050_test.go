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
