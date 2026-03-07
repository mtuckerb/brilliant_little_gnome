- **Typed contracts**: Added core game types and config constants with strict typing, ensuring compile-time safety and enabling TypeScript diagnostics.
- **Compile tests**: Created vitest suite that includes @ts-expect-error to validate type correctness.

- Implemented versioned storage adapter with namespaced keys and payload envelope to decouple persistence metadata from game state.
- **Verification**: Verified persistence logic via comprehensive Vitest suite covering default loads, save/load roundtrips, corruption recovery, and partial-data migration paths.