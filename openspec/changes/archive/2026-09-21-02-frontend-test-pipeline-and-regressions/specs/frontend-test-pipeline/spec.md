# Spec Delta

## Purpose

Automatically verify the frontend's type correctness and unit tests on every proposed change to `main`, so a regression in `frontend/src` is caught before merge instead of relying on someone remembering to run the checks locally.

## ADDED Requirements

### Requirement: Automatic frontend verification on pull requests
CI SHALL automatically run the frontend type check and the frontend unit test suite whenever a pull request targeting `main` is opened or updated, without requiring a manual trigger.

#### Scenario: A pull request is opened against main
- **WHEN** a pull request targeting `main` is opened or receives a new commit
- **THEN** the frontend type-check and unit-test jobs SHALL run automatically, with no manual dispatch required

### Requirement: CI fails on a frontend type error
CI SHALL fail the pull request check when `tsc --noEmit` reports any error in the frontend project.

#### Scenario: A type error is introduced
- **WHEN** a pull request introduces a TypeScript type error anywhere `tsc --noEmit -p frontend` covers
- **THEN** the CI check for that pull request SHALL fail

#### Scenario: No type errors
- **WHEN** a pull request introduces no TypeScript type error
- **THEN** the type-check step SHALL pass

### Requirement: CI fails on a failing frontend unit test
CI SHALL fail the pull request check when any test in the frontend unit test suite fails.

#### Scenario: A unit test fails
- **WHEN** a pull request causes any test under `frontend/tests/` to fail
- **THEN** the CI check for that pull request SHALL fail

#### Scenario: All unit tests pass
- **WHEN** every test under `frontend/tests/` passes
- **THEN** the unit-test step SHALL pass and SHALL NOT block the pull request
