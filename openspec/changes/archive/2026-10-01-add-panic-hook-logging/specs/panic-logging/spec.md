# panic-logging Delta Spec

## Purpose

Persist a durable, self-contained record of every Rust panic (message, source location, and backtrace) so a crash of the packaged application can be diagnosed after the process is gone, without a debugger or a minidump.

## ADDED Requirements

### Requirement: Panic capture is active from startup

The system SHALL install a process-wide panic hook early in application startup, before the main window and background workers are created, and it SHALL capture panics raised on any thread for the rest of the process lifetime.

#### Scenario: Panic on the main thread
- **WHEN** code running on the main thread panics after startup
- **THEN** the panic SHALL be recorded in the panic log

#### Scenario: Panic on a background thread
- **WHEN** a worker thread panics
- **THEN** the panic SHALL be recorded in the panic log, and the recording SHALL identify the thread

#### Scenario: Panic during startup
- **WHEN** a panic occurs after the hook is installed but before the application reaches its normal running state
- **THEN** the panic SHALL still be recorded

### Requirement: Panic record content

Each recorded panic SHALL include, at minimum: a timestamp, the application version, the process id, the thread name (or an explicit unknown-thread marker), the panic message, the source location as `file:line:column`, and a backtrace. When the panic payload is a string, that string SHALL be the recorded message; when the payload is not a string, the system SHALL still record the panic with a message that makes the payload type explicit.

#### Scenario: Panic with a string message
- **WHEN** code panics with a `&str` or `String` message such as `unwrap()` or `expect()` output
- **THEN** the recorded message SHALL contain that text verbatim

#### Scenario: Panic with a non-string payload
- **WHEN** code panics with a payload that is not a string
- **THEN** a record SHALL still be written, with a message indicating that the payload was not a string

#### Scenario: Source location is recorded
- **WHEN** the panic reports a source location
- **THEN** the record SHALL contain that file path, line, and column

#### Scenario: Backtrace is recorded
- **WHEN** any panic is recorded
- **THEN** the record SHALL contain a backtrace captured at the panic point, independent of any environment variable

### Requirement: Panic log location and persistence

The panic record SHALL be written to an append-only file named `panic.log` inside a `logs` directory under the application data directory. The system SHALL create the `logs` directory and the file when they do not exist. Each panic SHALL be appended as a delimited entry so that multiple panics in one process, or across processes, remain individually readable, and earlier entries SHALL NOT be overwritten or truncated.

#### Scenario: First panic creates the log
- **WHEN** the application panics and no `logs` directory or `panic.log` exists yet
- **THEN** the directory and file SHALL be created and the record written

#### Scenario: Repeated panics append
- **WHEN** the application panics more than once
- **THEN** each panic SHALL produce a new entry, and all previous entries SHALL remain present and unchanged

#### Scenario: Entries survive process exit
- **WHEN** a recorded panic ends the process
- **THEN** the written entry SHALL be readable on disk afterwards without additional flushing steps by the user

### Requirement: Panic logging never masks the original failure

Recording a panic SHALL be best-effort and SHALL NOT itself panic, block indefinitely, or change the process's normal panic behavior. If the log location cannot be created or written, the panic SHALL still be reported through the previously installed panic behavior and the application SHALL proceed with its normal panic/abort path. Previously installed panic output SHALL remain in effect so that console/stderr output in development builds is unchanged.

#### Scenario: Log path is not writable
- **WHEN** the `logs` directory or `panic.log` cannot be created or written
- **THEN** no secondary failure SHALL occur and the panic SHALL still be handled normally

#### Scenario: Existing panic output preserved
- **WHEN** the application runs with a console or attached stderr, such as a development run
- **THEN** the panic message SHALL still be emitted through the prior panic behavior in addition to being recorded

### Requirement: Release panics resolve to source lines

Release builds SHALL include line-table debug information for the application's own code so that recorded backtraces can resolve frames to `file:line` when the symbol file is present alongside the executable. When symbol information cannot be located at runtime, the record SHALL still be written with the available frame information, without degrading into an error.

#### Scenario: Release panic with symbols available
- **WHEN** a packaged or locally built release binary panics with its symbol file available next to the executable
- **THEN** the recorded backtrace SHALL include source file and line for application frames

#### Scenario: Symbols unavailable
- **WHEN** the symbol file is absent or unusable at runtime
- **THEN** the record SHALL still be written and the process behavior SHALL be unchanged

### Requirement: Panic records stay local

Panic recording SHALL write only to the local panic log. It SHALL NOT transmit the record, the message, or the backtrace over the network, and it SHALL NOT perform analytics or telemetry as part of panic handling.

#### Scenario: No network activity on panic
- **WHEN** a panic is recorded
- **THEN** no network request or telemetry event SHALL be produced by the panic-recording path
