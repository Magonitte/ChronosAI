# Chatterbox TTS API - Comprehensive Testing Suite

This directory contains a modernized, comprehensive testing suite for the Chatterbox TTS API using pytest.

## 🚀 Quick Start

### Prerequisites

1. **Install test dependencies:**

   ```bash
   # Using uv (recommended)
   uv pip install -e ".[test]"

   # Or using pip
   pip install -e ".[test]"
   ```

2. **Start the API server:**
   ```bash
   python main.py
   # API should be running at http://localhost:4123
   ```

### Running Tests

**Quick tests (recommended for development):**

```bash
python tests/run_tests.py --quick
```

**All tests (including slow tests):**

```bash
python tests/run_tests.py --all
```

**With coverage reporting:**

```bash
python tests/run_tests.py --coverage
```

## 📁 Test Structure

### Test Files

| File                    | Description                          | Markers          |
| ----------------------- | ------------------------------------ | ---------------- |
| `test_api.py`           | Core API functionality tests         | `api`            |
| `test_memory.py`        | Memory management and cleanup tests  | `memory`, `slow` |
| `test_regression.py`    | Backward compatibility tests         | `regression`     |
| `test_status.py`        | Status monitoring and tracking tests | `api`            |
| `test_voice_library.py` | Voice library management tests       | `voice`          |
| `test_voice_upload.py`  | Voice upload functionality tests     | `voice`          |

### Configuration Files

- `conftest.py` - Pytest configuration and shared fixtures
- `run_tests.py` - Comprehensive test runner with multiple options
- `README.md` - This documentation

## 🎯 Test Execution Modes

### By Test Type

```bash
# API functionality tests only
python tests/run_tests.py --api-only

# Memory management tests only
python tests/run_tests.py --memory

# Regression tests only
python tests/run_tests.py --regression

# Voice-related tests only
python tests/run_tests.py --voice

# Status monitoring tests only
python tests/run_tests.py --status
```

### By Speed

```bash
# Quick tests (default, excludes slow tests)
python tests/run_tests.py --quick

# All tests including slow/memory tests
python tests/run_tests.py --all
```

### Advanced Options

```bash
# Run tests in parallel (faster)
python tests/run_tests.py --parallel

# Generate HTML report
python tests/run_tests.py --html-report

# Run with coverage and generate reports
python tests/run_tests.py --coverage --html-report

# Test against different API URL
python tests/run_tests.py --api-url http://localhost:8080

# Run specific test by keyword
python tests/run_tests.py -k "test_health"

# Repeat tests for stability testing
python tests/run_tests.py --repeat 3

# Stop on first failure
python tests/run_tests.py --failfast

# Verbose output
python tests/run_tests.py --verbose
```

## 🔧 Direct Pytest Usage

You can also run pytest directly for more control:

```bash
# Run all tests
pytest tests/

# Run specific test file
pytest tests/test_api.py

# Run with markers
pytest -m "not slow"  # Exclude slow tests
pytest -m "api"       # Only API tests
pytest -m "memory"    # Only memory tests

# Run with coverage
pytest --cov=app --cov-report=html

# Run in parallel
pytest -n auto

# Verbose output
pytest -v

# Stop on first failure
pytest -x
```

## 📊 Test Markers

The test suite uses pytest markers to categorize tests:

- `slow` - Tests that take longer to run (memory tests, concurrent tests)
- `api` - API functionality tests
- `memory` - Memory management tests
- `voice` - Voice-related functionality tests
- `regression` - Backward compatibility tests
- `integration` - Integration tests
- `unit` - Unit tests

## 🧪 Test Categories

### API Tests (`test_api.py`)

- **Health endpoints** - `/health`, `/docs`, `/openapi.json`
- **TTS JSON endpoint** - `/v1/audio/speech`
- **TTS upload endpoint** - `/v1/audio/speech/upload`
- **Error handling** - Invalid inputs, parameter validation
- **Concurrent requests** - Multiple simultaneous requests
- **Performance metrics** - Response times, throughput

### Memory Tests (`test_memory.py`)

- **Memory tracking** - Request counting, memory monitoring
- **Sequential requests** - Memory usage over multiple requests
- **Concurrent memory usage** - Memory behavior with parallel requests
- **Memory cleanup** - Manual cleanup functionality
- **Memory limits** - Error conditions and edge cases

### Regression Tests (`test_regression.py`)

- **Backward compatibility** - Existing API behavior preservation
- **Parameter handling** - Parameter ranges and validation
- **Response formats** - Consistent response structures
- **Performance regression** - Response time monitoring
- **API structure** - Endpoint availability

### Voice Tests (`test_voice_library.py`, `test_voice_upload.py`)

- **Voice library management** - Upload, list, delete voices
- **Voice upload functionality** - File upload with speech generation
- **Error handling** - Invalid files, missing voices
- **Voice fallback** - Default voice usage

### Status Tests (`test_status.py`)

- **Status endpoints** - `/status`, `/status/progress`, `/status/statistics`
- **Progress tracking** - Real-time progress monitoring
- **Statistics collection** - Request statistics and metrics
- **Request history** - Historical request tracking

## 🔧 Configuration

### Environment Variables

- `CHATTERBOX_TEST_URL` - API URL for testing (default: `http://localhost:4123`)
- `TEST_TIMEOUT` - Test timeout in seconds (default: `60`)
- `API_HEALTH_TIMEOUT` - API health check timeout (default: `5`)

### Test Data

Test data is centralized in `conftest.py`:

- `TEST_TEXTS` - Various text samples for testing
- `TEST_PARAMETERS` - Common parameter combinations

## 📈 Reporting

### Coverage Reports

```bash
# Generate coverage report
python tests/run_tests.py --coverage

# Open coverage report
open htmlcov/index.html
```

### HTML Test Reports

```bash
# Generate HTML test report
python tests/run_tests.py --html-report

# Open test report
open test_report.html
```

### JUnit XML

```bash
# Generate JUnit XML for CI/CD
python tests/run_tests.py --junit-xml
```

## 🚀 CI/CD Integration

### GitHub Actions Example

```yaml
name: Tests
on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions/setup-python@v4
        with:
          python-version: '3.11'

      - name: Install dependencies
        run: |
          pip install -e ".[test]"
          # Or with uv (faster):
          # uv sync --group test

      - name: Start API
        run: |
          python main.py &
          sleep 10

      - name: Run tests
        run: |
          python tests/run_tests.py --coverage --junit-xml

      - name: Upload coverage
        uses: codecov/codecov-action@v3
```

## 🔍 Debugging Tests

### Running Individual Tests

```bash
# Run specific test method
pytest tests/test_api.py::TestHealthEndpoints::test_health_check

# Run specific test class
pytest tests/test_api.py::TestHealthEndpoints

# Run with debugging output
pytest -v -s tests/test_api.py

# Drop into debugger on failure
pytest --pdb tests/test_api.py
```

### Test Output Files

Test output files are saved to `test_outputs/` directory:

- Audio files generated during tests
- Logs and debugging information
- Temporary test data

## 🛠️ Adding New Tests

### Creating a New Test File

1. Create `tests/test_newfeature.py`
2. Import required fixtures from `conftest.py`
3. Add appropriate pytest markers
4. Follow existing patterns for test organization

Example:

```python
import pytest
from conftest import generate_speech_and_validate, TEST_TEXTS

class TestNewFeature:
    """Test new feature functionality"""

    @pytest.mark.api
    def test_new_endpoint(self, api_client):
        """Test new API endpoint"""
        response = api_client.get("/new-endpoint")
        assert response.status_code == 200
```

### Adding Test Fixtures

Add shared fixtures to `conftest.py`:

```python
@pytest.fixture
def new_test_fixture():
    """Provide test data for new feature"""
    return {"test": "data"}
```

## 📝 Best Practices

1. **Use descriptive test names** - Test names should clearly describe what is being tested
2. **Group related tests** - Use test classes to organize related functionality
3. **Use appropriate markers** - Mark slow tests, categorize by functionality
4. **Clean up resources** - Use fixtures for setup/teardown
5. **Test error conditions** - Include negative test cases
6. **Document complex tests** - Add docstrings for complex test logic
7. **Use parametrized tests** - Test multiple scenarios efficiently

## 🐛 Troubleshooting

### Common Issues

**API not responding:**

```bash
# Check if API is running
curl http://localhost:4123/health

# Start API if needed
python main.py
```

**Tests timing out:**

```bash
# Increase timeout
python tests/run_tests.py --timeout 120
```

**Memory tests failing:**

```bash
# Run memory tests with more time
python tests/run_tests.py --memory --timeout 180
```

**Parallel tests unstable:**

```bash
# Run sequentially
python tests/run_tests.py --all
```

### Getting Help

1. Check test output for specific error messages
2. Run with `--verbose` for detailed output
3. Use `--failfast` to stop on first failure
4. Check API logs for server-side issues
5. Verify test environment and dependencies

## 📚 References

- [pytest documentation](https://docs.pytest.org/)
- [pytest-cov documentation](https://pytest-cov.readthedocs.io/)
- [Chatterbox TTS API documentation](../docs/API_README.md)
