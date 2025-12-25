// internal/types/manifest.go
package types

import "time"

// Manifest represents archived data metadata for a feed/date
type Manifest struct {
	Feed             string       `json:"feed" yaml:"feed"`
	Date             string       `json:"date" yaml:"date"`
	Format           string       `json:"format" yaml:"format"`
	RotationInterval string       `json:"rotation_interval" yaml:"rotation_interval"`
	Files            []FileEntry  `json:"files" yaml:"files"`
	Gaps             []Gap        `json:"gaps" yaml:"gaps"`
	Tickers          []string     `json:"tickers" yaml:"tickers"`
	MessageTypes     []string     `json:"message_types" yaml:"message_types"`
	HasGaps          bool         `json:"has_gaps" yaml:"has_gaps"`
}

// FileEntry represents a single archived file
type FileEntry struct {
	Name         string    `json:"name" yaml:"name"`
	Start        time.Time `json:"start" yaml:"start"`
	End          time.Time `json:"end" yaml:"end"`
	Records      uint64    `json:"records" yaml:"records"`
	Bytes        uint64    `json:"bytes" yaml:"bytes"`
	NatsStartSeq uint64    `json:"nats_start_seq" yaml:"nats_start_seq"`
	NatsEndSeq   uint64    `json:"nats_end_seq" yaml:"nats_end_seq"`
}

// Gap represents a detected gap in the data stream
type Gap struct {
	AfterSeq     uint64    `json:"after_seq" yaml:"after_seq"`
	MissingCount uint64    `json:"missing_count" yaml:"missing_count"`
	DetectedAt   time.Time `json:"detected_at" yaml:"detected_at"`
}

// TotalRecords returns the sum of records across all files
func (m *Manifest) TotalRecords() uint64 {
	var total uint64
	for _, f := range m.Files {
		total += f.Records
	}
	return total
}

// TotalBytes returns the sum of bytes across all files
func (m *Manifest) TotalBytes() uint64 {
	var total uint64
	for _, f := range m.Files {
		total += f.Bytes
	}
	return total
}
