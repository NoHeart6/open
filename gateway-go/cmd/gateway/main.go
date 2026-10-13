package main

import (
	"log"
	"net/http"
	"os"

	"opencommander/gateway/internal/api"
	"opencommander/gateway/internal/tasks"
)

func main() {
	store := tasks.NewStore()
	h := api.New(store)
	mux := http.NewServeMux()
	h.Routes(mux)
	port := os.Getenv("PORT")
	if port == "" {
		port = "8080"
	}
	log.Printf("gateway on :%s", port)
	log.Fatal(http.ListenAndServe(":"+port, mux))
}
