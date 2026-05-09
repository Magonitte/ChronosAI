#!/usr/bin/env python3
"""
Comprehensive test runner for Chatterbox TTS API tests

This script provides different test execution modes and reporting options.
"""

import sys
import os
import argparse
import subprocess
from pathlib import Path
from typing import List, Optional

# Add project root to path
PROJECT_ROOT = Path(__file__).parent.parent
sys.path.insert(0, str(PROJECT_ROOT))


def run_command(cmd: List[str], description: str) -> int:
    """Run a command and return exit code"""
    print(f"\n🔧 {description}")
    print(f"Command: {' '.join(cmd)}")
    print("-" * 60)
    
    try:
        result = subprocess.run(cmd, cwd=PROJECT_ROOT)
        return result.returncode
    except Exception as e:
        print(f"Error running command: {e}")
        return 1


def check_api_health(api_url: str) -> bool:
    """Check if API is healthy before running tests"""
    try:
        import requests
        response = requests.get(f"{api_url}/health", timeout=5)
        return response.status_code == 200
    except Exception:
        return False


def main():
    parser = argparse.ArgumentParser(
        description="Run Chatterbox TTS API tests",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  %(prog)s --quick                    # Run quick tests only
  %(prog)s --all                      # Run all tests including slow ones
  %(prog)s --api-only                 # Run only API tests
  %(prog)s --memory                   # Run only memory tests
  %(prog)s --regression               # Run only regression tests
  %(prog)s --coverage                 # Run with coverage reporting
  %(prog)s --parallel                 # Run tests in parallel
  %(prog)s --api-url http://localhost:8080  # Use different API URL
        """
    )
    
    # Test selection options
    test_group = parser.add_mutually_exclusive_group()
    test_group.add_argument(
        "--quick", action="store_true",
        help="Run only quick tests (excludes slow/memory tests)"
    )
    test_group.add_argument(
        "--all", action="store_true", 
        help="Run all tests including slow ones"
    )
    test_group.add_argument(
        "--api-only", action="store_true",
        help="Run only API functionality tests"
    )
    test_group.add_argument(
        "--memory", action="store_true",
        help="Run only memory management tests"
    )
    test_group.add_argument(
        "--regression", action="store_true", 
        help="Run only regression tests"
    )
    test_group.add_argument(
        "--voice", action="store_true",
        help="Run only voice-related tests"
    )
    test_group.add_argument(
        "--status", action="store_true",
        help="Run only status/monitoring tests"
    )
    
    # Execution options
    parser.add_argument(
        "--coverage", action="store_true",
        help="Run tests with coverage reporting"
    )
    parser.add_argument(
        "--parallel", action="store_true",
        help="Run tests in parallel (faster but may be less stable)"
    )
    parser.add_argument(
        "--verbose", "-v", action="store_true",
        help="Verbose output"
    )
    parser.add_argument(
        "--failfast", action="store_true",
        help="Stop on first failure"
    )
    parser.add_argument(
        "--no-header", action="store_true",
        help="Skip test header/summary"
    )
    
    # Configuration options
    parser.add_argument(
        "--api-url", default="http://localhost:4123",
        help="API URL for testing (default: http://localhost:4123)"
    )
    parser.add_argument(
        "--timeout", type=int, default=60,
        help="Test timeout in seconds (default: 60)"
    )
    parser.add_argument(
        "--output-dir", default="test_outputs",
        help="Directory for test output files (default: test_outputs)"
    )
    
    # Reporting options
    parser.add_argument(
        "--html-report", action="store_true",
        help="Generate HTML test report"
    )
    parser.add_argument(
        "--junit-xml", action="store_true",
        help="Generate JUnit XML report"
    )
    
    # Advanced options
    parser.add_argument(
        "--repeat", type=int, default=1,
        help="Repeat tests N times (useful for stability testing)"
    )
    parser.add_argument(
        "--marker", action="append",
        help="Only run tests with specific markers (can be used multiple times)"
    )
    parser.add_argument(
        "--keyword", "-k",
        help="Only run tests matching given substring expression"
    )
    
    args = parser.parse_args()
    
    # Set environment variables
    os.environ["CHATTERBOX_TEST_URL"] = args.api_url
    os.environ["TEST_TIMEOUT"] = str(args.timeout)
    
    if not args.no_header:
        print("=" * 80)
        print("🧪 Chatterbox TTS API - Comprehensive Test Suite")
        print("=" * 80)
        print(f"API URL: {args.api_url}")
        print(f"Test timeout: {args.timeout}s")
        print(f"Output directory: {args.output_dir}")
        
        # Check API health
        print(f"\n🔍 Checking API health...")
        if check_api_health(args.api_url):
            print("✅ API is healthy and responding")
        else:
            print("❌ API is not responding or unhealthy")
            print("   Make sure the API is running before running tests")
            response = input("Continue anyway? (y/N): ")
            if response.lower() != 'y':
                return 1
    
    # Build pytest command
    cmd = ["python", "-m", "pytest"]
    
    # Add test directory
    cmd.append("tests/")
    
    # Test selection
    if args.quick:
        cmd.extend(["-m", "not slow"])
        print("📋 Mode: Quick tests (excluding slow tests)")
    elif args.all:
        cmd.append("--runslow")
        print("📋 Mode: All tests (including slow tests)")
    elif args.api_only:
        cmd.extend(["-m", "api"])
        print("📋 Mode: API tests only")
    elif args.memory:
        cmd.extend(["-m", "memory", "--runslow"])
        print("📋 Mode: Memory tests only")
    elif args.regression:
        cmd.extend(["-m", "regression"]) 
        print("📋 Mode: Regression tests only")
    elif args.voice:
        cmd.extend(["-m", "voice"])
        print("📋 Mode: Voice tests only")
    elif args.status:
        cmd.extend(["tests/test_status.py"])
        print("📋 Mode: Status tests only")
    else:
        # Default: run quick tests
        cmd.extend(["-m", "not slow"])
        print("📋 Mode: Default (quick tests only, use --all for everything)")
    
    # Add custom markers
    if args.marker:
        for marker in args.marker:
            cmd.extend(["-m", marker])
    
    # Add keyword filtering
    if args.keyword:
        cmd.extend(["-k", args.keyword])
    
    # Execution options
    if args.verbose:
        cmd.append("-v")
    
    if args.failfast:
        cmd.append("-x")
    
    if args.parallel:
        cmd.extend(["-n", "auto"])
        print("⚡ Parallel execution enabled")
    
    # Coverage options
    if args.coverage:
        cmd.extend([
            "--cov=app",
            "--cov-report=term-missing",
            "--cov-report=html:htmlcov",
        ])
        print("📊 Coverage reporting enabled")
    
    # Reporting options
    if args.html_report:
        cmd.extend(["--html=test_report.html", "--self-contained-html"])
        print("📄 HTML report will be generated")
    
    if args.junit_xml:
        cmd.extend(["--junit-xml=test_results.xml"])
        print("📋 JUnit XML report will be generated")
    
    # Repeat tests
    if args.repeat > 1:
        cmd.extend(["--count", str(args.repeat)])
        print(f"🔄 Tests will be repeated {args.repeat} times")
    
    # Run tests
    exit_code = 0
    for run in range(args.repeat):
        if args.repeat > 1:
            print(f"\n🔄 Test run {run + 1}/{args.repeat}")
        
        result = run_command(cmd, f"Running pytest")
        if result != 0:
            exit_code = result
            if args.failfast:
                break
    
    # Summary
    if not args.no_header:
        print("\n" + "=" * 80)
        print("📊 Test Execution Summary")
        print("=" * 80)
        
        if exit_code == 0:
            print("🎉 All tests passed!")
        else:
            print(f"❌ Tests failed with exit code: {exit_code}")
        
        print(f"\n📂 Generated files:")
        generated_files = []
        
        if args.coverage:
            if Path("htmlcov/index.html").exists():
                generated_files.append("  📊 Coverage report: htmlcov/index.html")
        
        if args.html_report:
            if Path("test_report.html").exists():
                generated_files.append("  📄 HTML test report: test_report.html")
        
        if args.junit_xml:
            if Path("test_results.xml").exists():
                generated_files.append("  📋 JUnit XML: test_results.xml")
        
        if Path("test_outputs").exists() and any(Path("test_outputs").iterdir()):
            generated_files.append(f"  🎵 Audio outputs: test_outputs/")
        
        if generated_files:
            print("\n".join(generated_files))
        else:
            print("  (no additional files generated)")
        
        print(f"\n💡 Useful commands:")
        print(f"  Quick tests:     python tests/run_tests.py --quick")
        print(f"  All tests:       python tests/run_tests.py --all")
        print(f"  With coverage:   python tests/run_tests.py --coverage")
        print(f"  Memory tests:    python tests/run_tests.py --memory")
        print(f"  API docs:        open {args.api_url}/docs")
    
    return exit_code


if __name__ == "__main__":
    exit(main()) 