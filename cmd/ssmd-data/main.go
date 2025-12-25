// cmd/ssmd-data/main.go
package main

import (
	"log"
	"net/http"
	"os"

	"github.com/aaronwald/ssmd/internal/api"
	"github.com/aaronwald/ssmd/internal/data"
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

	log.Printf("ssmd-data listening on :%s", port)
	if err := http.ListenAndServe(":"+port, server); err != nil {
		log.Fatal(err)
	}
}
