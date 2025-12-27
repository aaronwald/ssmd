// cmd/ssmd-data/main.go
package main

import (
	"database/sql"
	"log"
	"net/http"
	"os"

	"github.com/aaronwald/ssmd/internal/api"
	"github.com/aaronwald/ssmd/internal/data"
	"github.com/aaronwald/ssmd/internal/secmaster"
	_ "github.com/lib/pq"
)

func main() {
	dataPath := os.Getenv("SSMD_DATA_PATH")
	if dataPath == "" {
		log.Fatal("SSMD_DATA_PATH required")
	}

	apiKey := os.Getenv("SSMD_API_KEY")
	if apiKey == "" {
		log.Fatal("SSMD_API_KEY required")
	}

	port := os.Getenv("PORT")
	if port == "" {
		port = "8080"
	}

	storage, err := data.NewStorage(dataPath)
	if err != nil {
		log.Fatalf("creating storage: %v", err)
	}

	server := api.NewServer(storage, apiKey)

	// Optional: Connect to PostgreSQL for secmaster endpoints
	if dbURL := os.Getenv("DATABASE_URL"); dbURL != "" {
		db, err := sql.Open("postgres", dbURL)
		if err != nil {
			log.Fatalf("connecting to database: %v", err)
		}
		defer db.Close()

		if err := db.Ping(); err != nil {
			log.Fatalf("pinging database: %v", err)
		}

		server.SetSecmasterStore(secmaster.NewStore(db))
		log.Printf("secmaster endpoints enabled (PostgreSQL connected)")
	}

	log.Printf("ssmd-data listening on :%s", port)
	if err := http.ListenAndServe(":"+port, server); err != nil {
		log.Fatal(err)
	}
}
