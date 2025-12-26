// internal/api/server.go
package api

import (
	"encoding/json"
	"log"
	"net/http"

	"github.com/aaronwald/ssmd/internal/data"
)

// APIVersion is the current API version. Increment when adding new endpoints.
const APIVersion = "0.2.1"

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
	s.mux.HandleFunc("GET /version", s.handleVersion)
	s.mux.HandleFunc("GET /datasets", s.requireAPIKey(s.handleDatasets))
	s.mux.HandleFunc("GET /datasets/{feed}/{date}/sample", s.requireAPIKey(s.handleSample))
	s.mux.HandleFunc("GET /datasets/{feed}/{date}/tickers", s.requireAPIKey(s.handleTickers))
	s.mux.HandleFunc("GET /schema/{feed}/{type}", s.requireAPIKey(s.handleSchema))
	s.mux.HandleFunc("GET /builders", s.requireAPIKey(s.handleBuilders))
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

func (s *Server) handleVersion(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"version": APIVersion})
}
