# ARM SMMU v3 C++11 Implementation

A comprehensive software model of the ARM System Memory Management Unit (SMMU) version 3, implemented in strict C++11 compliance for development, simulation, and testing environments.

## Features

- **Stream-based architecture** with unique StreamIDs and PASID support
- **Two-stage address translation** (Stage-1 and Stage-2)
- **Comprehensive fault handling** with Terminate and Stall modes
- **TLB caching** for performance optimization
- **Sparse page table representation** for memory efficiency
- **Thread-safe event queue** management
- **C++11 strict compliance** with no external dependencies

## Building

```bash
mkdir -p build && cd build
cmake .. -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_STANDARD=11
make -j$(nproc)
```

## Testing

```bash
cd build
make test
# or
ctest --output-on-failure
```

## Documentation

See `ARM_SMMU_v3_PRD.md` for complete product requirements and `TASKS.md` for implementation progress.

## License

Copyright (c) 2025 John Greninger. All rights reserved.
