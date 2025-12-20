package types

// Versioned is an interface for types that have version information with effective dates
type Versioned interface {
	GetEffectiveFrom() string
}

// SortVersionsDesc sorts a slice of Versioned items by EffectiveFrom in descending order
// Returns the sorted indices
func SortVersionsDesc[T Versioned](versions []T) []T {
	sorted := make([]T, len(versions))
	copy(sorted, versions)

	// Simple bubble sort for small slices (typically < 10 versions)
	for i := 0; i < len(sorted)-1; i++ {
		for j := 0; j < len(sorted)-i-1; j++ {
			if sorted[j].GetEffectiveFrom() < sorted[j+1].GetEffectiveFrom() {
				sorted[j], sorted[j+1] = sorted[j+1], sorted[j]
			}
		}
	}
	return sorted
}
