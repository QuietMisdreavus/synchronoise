# changelog for synchronoise

## Pending
### Added
- `CountdownEvent::guard` and `CountdownGuard`, to provide scope-based increment/decrement
- `WriterReaderPhaser`, a primitive that allows multiple wait-free "writer critical sections"
  against a "reader phase flip" that waits for currently-active writers to finish
  - also the related structs `PhaserCriticalSection` and `PhaserReadLock`

## [0.2.0] - 2017-02-28
### Added
- `CountdownEvent::wait_timeout`, to wait but also have a timeout
- `SignalEvent`, a primitive that lets one or more threads wait for a signal from another one

## [0.1.0] - 2017-02-27
### Added
- `CountdownEvent`, a primitive that keeps a counter that allows you to block until it hits zero

<!-- vim: set tw=100 expandtab: -->
