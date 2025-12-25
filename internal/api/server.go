// internal/api/server.go
package api

import (
	"encoding/json"
	"log"
	"net/http"

	"github.com/aaronwald/ssmd/internal/data"
)

type Server struct {
	storage data.Storage
	apiKey  string
	mux     *http.ServeMux
}

func NewServer(storage data.Storage, apiKey string) *Server {
	s := &Server{
		storage: storage,
		apiKey:  apiKey,
		mux:     http.NewServeMux(),
	}
	s.routes()
	return s
}

func (s *Server) routes() {
	s.mux.HandleFunc("GET /health", s.handleHealth)
	s.mux.HandleFunc("GET /datasets", s.requireAPIKey(s.handleDatasets))
	s.mux.HandleFunc("GET /datasets/{feed}/{date}/sample", s.requireAPIKey(s.handleSample))
}

func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	// Log all requests
	log.Printf("%s %s", r.Method, r.URL.Path)
	s.mux.ServeHTTP(w, r)
}

func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "ok"})
}
