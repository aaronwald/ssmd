package types

import (
	"cmp"
	"slices"
)

// Versioned is an interface for types that have version information with effective dates
type Versioned interface {
	GetEffectiveFrom() string
}

// SortVersionsDesc sorts a slice of Versioned items by EffectiveFrom in descending order
func SortVersionsDesc[T Versioned](versions []T) []T {
	sorted := make([]T, len(versions))
	copy(sorted, versions)

	slices.SortFunc(sorted, func(a, b T) int {
		return cmp.Compare(b.GetEffectiveFrom(), a.GetEffectiveFrom())
	})
	return sorted
}
