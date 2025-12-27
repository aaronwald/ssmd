// internal/api/server.go
package api

import (
	"encoding/json"
	"log"
	"net/http"

	"github.com/aaronwald/ssmd/internal/data"
	"github.com/aaronwald/ssmd/internal/secmaster"
)

// APIVersion is the current API version. Increment when adding new endpoints.
const APIVersion = "0.3.0"

type Server struct {
	storage        data.Storage
	apiKey         string
	mux            *http.ServeMux
	secmasterStore *secmaster.Store
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

// SetSecmasterStore sets the optional secmaster store for market queries
func (s *Server) SetSecmasterStore(store *secmaster.Store) {
	s.secmasterStore = store
}

func (s *Server) routes() {
	s.mux.HandleFunc("GET /health", s.handleHealth)
	s.mux.HandleFunc("GET /version", s.handleVersion)
	s.mux.HandleFunc("GET /datasets", s.requireAPIKey(s.handleDatasets))
	s.mux.HandleFunc("GET /datasets/{feed}/{date}/sample", s.requireAPIKey(s.handleSample))
	s.mux.HandleFunc("GET /datasets/{feed}/{date}/tickers", s.requireAPIKey(s.handleTickers))
	s.mux.HandleFunc("GET /schema/{feed}/{type}", s.requireAPIKey(s.handleSchema))
	s.mux.HandleFunc("GET /builders", s.requireAPIKey(s.handleBuilders))
	// Secmaster endpoints
	s.mux.HandleFunc("GET /markets", s.requireAPIKey(s.handleMarkets))
	s.mux.HandleFunc("GET /markets/{ticker}", s.requireAPIKey(s.handleMarket))
	s.mux.HandleFunc("GET /fees", s.requireAPIKey(s.handleFees))
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
