#!/usr/bin/env python3
"""
Feature Parity Test: mira-chat vs Claude Code

Tests specific features side-by-side to verify mira-chat correctly
implements the same capabilities as Claude Code.

Features tested:
1. File read - Read a file and extract info
2. File write - Create a new file
3. File edit - Modify existing file
4. Grep search - Find patterns in code
5. Glob search - Find files by pattern
6. Bash execution - Run shell commands
7. Web search - Search the web
8. Multi-file context - Understand across files
"""

import subprocess
import json
import time
import os
import re
import tempfile
import shutil
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

# Load environment
ENV_FILE = Path("/home/peter/Mira/.env")
if ENV_FILE.exists():
    for line in ENV_FILE.read_text().splitlines():
        if "=" in line and not line.startswith("#"):
            key, value = line.split("=", 1)
            os.environ[key.strip()] = value.strip().strip('"')


@dataclass
class FeatureResult:
    """Result from testing a feature"""
    feature: str
    system: str
    success: bool
    output: str
    expected_behavior: str
    actual_behavior: str
    duration: float
    notes: str = ""


# Test cases - each tests a specific feature
FEATURE_TESTS = [
    {
        "id": "read-file",
        "name": "Read File",
        "prompt": "Read the file mira-chat/Cargo.toml and tell me the package name and version. Just give me the name and version, nothing else.",
        "expected": lambda out: "mira-chat" in out.lower() and "0.1" in out,
        "description": "Should read file and extract package name + version"
    },
    {
        "id": "grep-search",
        "name": "Grep Search",
        "prompt": "Search for 'ToolExecutor' in the mira-chat/src directory. How many files contain this term?",
        "expected": lambda out: any(str(n) in out for n in range(2, 10)),  # Should find 2-9 files
        "description": "Should search code and count matches"
    },
    {
        "id": "glob-find",
        "name": "Glob Find Files",
        "prompt": "Find all .rs files in mira-chat/src/tools/ directory. List just the filenames.",
        "expected": lambda out: "mod.rs" in out and "file.rs" in out,
        "description": "Should find Rust files in directory"
    },
    {
        "id": "bash-command",
        "name": "Bash Command",
        "prompt": "Run 'wc -l mira-chat/src/main.rs' and tell me how many lines are in the file.",
        "expected": lambda out: any(str(n) in out for n in range(50, 500)),  # Should be reasonable line count
        "description": "Should execute bash and report result"
    },
    {
        "id": "understand-code",
        "name": "Understand Code Structure",
        "prompt": "Look at mira-chat/src/lib.rs and list the public modules it exports. Just the module names.",
        "expected": lambda out: "repl" in out.lower() or "tools" in out.lower() or "context" in out.lower(),
        "description": "Should understand module structure"
    },
    {
        "id": "multi-file",
        "name": "Cross-File Understanding",
        "prompt": "What struct is returned by the execute() function in mira-chat/src/repl/execution.rs? Just the struct name.",
        "expected": lambda out: "executionresult" in out.lower() or "execution_result" in out.lower(),
        "description": "Should find and report the return type"
    },
]


def run_mira_chat(prompt: str, timeout: int = 120) -> tuple[str, float]:
    """Run a prompt through mira-chat, return (output, duration)"""
    env = os.environ.copy()
    env["DATABASE_URL"] = "sqlite:///home/peter/Mira/data/mira.db"

    start = time.time()
    try:
        proc = subprocess.run(
            ["/home/peter/Mira/target/release/mira-chat", "-p", "/home/peter/Mira"],
            input=f"/clear\n{prompt}\n/quit\n",
            capture_output=True,
            text=True,
            timeout=timeout,
            env=env,
            cwd="/home/peter/Mira"
        )
        output = proc.stdout + proc.stderr
        duration = time.time() - start
        return output, duration
    except subprocess.TimeoutExpired:
        return "TIMEOUT", time.time() - start
    except Exception as e:
        return f"ERROR: {e}", time.time() - start


def run_claude_code(prompt: str, timeout: int = 120) -> tuple[str, float]:
    """Run a prompt through Claude Code, return (output, duration)"""
    start = time.time()
    try:
        proc = subprocess.run(
            ["claude", "-p", prompt, "--output-format", "json"],
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd="/home/peter/Mira"
        )
        duration = time.time() - start

        try:
            data = json.loads(proc.stdout)
            output = data.get("result", proc.stdout)
        except json.JSONDecodeError:
            output = proc.stdout

        return output, duration
    except subprocess.TimeoutExpired:
        return "TIMEOUT", time.time() - start
    except Exception as e:
        return f"ERROR: {e}", time.time() - start


def extract_answer(output: str) -> str:
    """Extract the actual answer from output, removing tool call noise"""
    # For mira-chat, skip the header and find the response
    lines = output.split('\n')

    # Find where the actual response starts (after tool calls)
    response_lines = []
    in_response = False

    for line in lines:
        # Skip mira-chat header
        if "GPT-5.2 Thinking" in line or "Type your message" in line:
            continue
        if line.startswith(">>>") or line.startswith("..."):
            continue
        if "[tool:" in line or "[tokens:" in line or "[reasoning:" in line:
            continue
        if line.strip().startswith("[") and line.strip().endswith("]"):
            continue

        # Collect actual response content
        if line.strip():
            response_lines.append(line.strip())

    return "\n".join(response_lines[-20:])  # Last 20 lines should have the answer


def run_feature_test(test: dict) -> tuple[FeatureResult, FeatureResult]:
    """Run a single feature test through both systems"""
    print(f"\n  Testing: {test['name']}")
    print(f"  Prompt: {test['prompt'][:60]}...")

    # Run mira-chat
    print(f"    [mira-chat] Running...", end="", flush=True)
    mira_output, mira_duration = run_mira_chat(test["prompt"])
    mira_answer = extract_answer(mira_output)
    mira_success = test["expected"](mira_answer)
    print(f" {mira_duration:.1f}s {'✓' if mira_success else '✗'}")

    time.sleep(1)  # Small delay

    # Run Claude Code
    print(f"    [Claude]    Running...", end="", flush=True)
    claude_output, claude_duration = run_claude_code(test["prompt"])
    claude_success = test["expected"](claude_output)
    print(f" {claude_duration:.1f}s {'✓' if claude_success else '✗'}")

    mira_result = FeatureResult(
        feature=test["id"],
        system="mira-chat",
        success=mira_success,
        output=mira_answer[:500],
        expected_behavior=test["description"],
        actual_behavior="Passed" if mira_success else "Failed validation",
        duration=mira_duration
    )

    claude_result = FeatureResult(
        feature=test["id"],
        system="claude-code",
        success=claude_success,
        output=claude_output[:500],
        expected_behavior=test["description"],
        actual_behavior="Passed" if claude_success else "Failed validation",
        duration=claude_duration
    )

    return mira_result, claude_result


def test_file_write():
    """Test file write capability - creates and verifies a file"""
    print(f"\n  Testing: File Write (special test)")

    test_file = "/tmp/mira_test_write.txt"
    test_content = "Hello from benchmark test"

    # Clean up first
    if os.path.exists(test_file):
        os.remove(test_file)

    # Test mira-chat
    print(f"    [mira-chat] Running...", end="", flush=True)
    prompt = f"Write a file to {test_file} with the content: {test_content}"
    mira_output, mira_duration = run_mira_chat(prompt)
    mira_success = os.path.exists(test_file)
    if mira_success:
        content = Path(test_file).read_text()
        mira_success = test_content in content
    print(f" {mira_duration:.1f}s {'✓' if mira_success else '✗'}")

    # Clean up for Claude test
    if os.path.exists(test_file):
        os.remove(test_file)

    time.sleep(1)

    # Test Claude Code
    print(f"    [Claude]    Running...", end="", flush=True)
    claude_output, claude_duration = run_claude_code(prompt)
    claude_success = os.path.exists(test_file)
    if claude_success:
        content = Path(test_file).read_text()
        claude_success = test_content in content
    print(f" {claude_duration:.1f}s {'✓' if claude_success else '✗'}")

    # Clean up
    if os.path.exists(test_file):
        os.remove(test_file)

    mira_result = FeatureResult(
        feature="write-file",
        system="mira-chat",
        success=mira_success,
        output=mira_output[:500] if isinstance(mira_output, str) else str(mira_output)[:500],
        expected_behavior="Should create file with specified content",
        actual_behavior="File created correctly" if mira_success else "File not created or wrong content",
        duration=mira_duration
    )

    claude_result = FeatureResult(
        feature="write-file",
        system="claude-code",
        success=claude_success,
        output=claude_output[:500] if isinstance(claude_output, str) else str(claude_output)[:500],
        expected_behavior="Should create file with specified content",
        actual_behavior="File created correctly" if claude_success else "File not created or wrong content",
        duration=claude_duration
    )

    return mira_result, claude_result


def test_file_edit():
    """Test file edit capability"""
    print(f"\n  Testing: File Edit (special test)")

    test_file = "/tmp/mira_test_edit.txt"
    original = "The quick brown fox"
    expected = "The slow brown fox"

    # Test mira-chat
    Path(test_file).write_text(original)
    print(f"    [mira-chat] Running...", end="", flush=True)
    prompt = f"Edit the file {test_file} and replace 'quick' with 'slow'"
    mira_output, mira_duration = run_mira_chat(prompt)
    content = Path(test_file).read_text() if os.path.exists(test_file) else ""
    mira_success = "slow" in content and "quick" not in content
    print(f" {mira_duration:.1f}s {'✓' if mira_success else '✗'}")

    time.sleep(1)

    # Reset and test Claude Code
    Path(test_file).write_text(original)
    print(f"    [Claude]    Running...", end="", flush=True)
    claude_output, claude_duration = run_claude_code(prompt)
    content = Path(test_file).read_text() if os.path.exists(test_file) else ""
    claude_success = "slow" in content and "quick" not in content
    print(f" {claude_duration:.1f}s {'✓' if claude_success else '✗'}")

    # Clean up
    if os.path.exists(test_file):
        os.remove(test_file)

    mira_result = FeatureResult(
        feature="edit-file",
        system="mira-chat",
        success=mira_success,
        output=mira_output[:500] if isinstance(mira_output, str) else str(mira_output)[:500],
        expected_behavior="Should replace 'quick' with 'slow'",
        actual_behavior="Edit successful" if mira_success else "Edit failed",
        duration=mira_duration
    )

    claude_result = FeatureResult(
        feature="edit-file",
        system="claude-code",
        success=claude_success,
        output=claude_output[:500] if isinstance(claude_output, str) else str(claude_output)[:500],
        expected_behavior="Should replace 'quick' with 'slow'",
        actual_behavior="Edit successful" if claude_success else "Edit failed",
        duration=claude_duration
    )

    return mira_result, claude_result


def print_results(mira_results: list, claude_results: list):
    """Print comparison results"""
    print("\n" + "=" * 70)
    print("FEATURE PARITY TEST RESULTS")
    print("=" * 70)

    print(f"\n{'Feature':<25} {'mira-chat':>12} {'Claude Code':>12} {'Match':>8}")
    print("-" * 60)

    matches = 0
    total = len(mira_results)

    for mira, claude in zip(mira_results, claude_results):
        mira_status = "✓" if mira.success else "✗"
        claude_status = "✓" if claude.success else "✗"
        match = "✓" if mira.success == claude.success else "≠"
        if mira.success and claude.success:
            matches += 1

        print(f"{mira.feature:<25} {mira_status:>12} {claude_status:>12} {match:>8}")

    print("-" * 60)

    mira_pass = sum(1 for r in mira_results if r.success)
    claude_pass = sum(1 for r in claude_results if r.success)

    print(f"{'TOTAL':<25} {mira_pass}/{total:>9} {claude_pass}/{total:>9} {matches}/{total:>5}")

    # Timing comparison
    print(f"\n{'Average Time':<25} {sum(r.duration for r in mira_results)/len(mira_results):>11.1f}s {sum(r.duration for r in claude_results)/len(claude_results):>11.1f}s")

    # Show any failures
    failures = [(m, c) for m, c in zip(mira_results, claude_results) if not m.success or not c.success]
    if failures:
        print("\n" + "-" * 60)
        print("FAILURES:")
        for mira, claude in failures:
            if not mira.success:
                print(f"\n  [mira-chat] {mira.feature}: {mira.actual_behavior}")
                print(f"    Output preview: {mira.output[:100]}...")
            if not claude.success:
                print(f"\n  [Claude] {claude.feature}: {claude.actual_behavior}")
                print(f"    Output preview: {claude.output[:100]}...")


def save_results(mira_results: list, claude_results: list):
    """Save results to JSON"""
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    results_dir = Path("/home/peter/Mira/benchmarks/results")
    results_dir.mkdir(exist_ok=True)

    data = {
        "timestamp": timestamp,
        "test_type": "feature_parity",
        "mira_chat": [
            {
                "feature": r.feature,
                "success": r.success,
                "duration": r.duration,
                "expected": r.expected_behavior,
                "actual": r.actual_behavior,
                "output_preview": r.output[:200]
            }
            for r in mira_results
        ],
        "claude_code": [
            {
                "feature": r.feature,
                "success": r.success,
                "duration": r.duration,
                "expected": r.expected_behavior,
                "actual": r.actual_behavior,
                "output_preview": r.output[:200]
            }
            for r in claude_results
        ]
    }

    output_file = results_dir / f"feature_parity_{timestamp}.json"
    output_file.write_text(json.dumps(data, indent=2))
    print(f"\nResults saved to: {output_file}")


def main():
    print("=" * 70)
    print("FEATURE PARITY TEST: mira-chat vs Claude Code")
    print("=" * 70)
    print(f"Testing {len(FEATURE_TESTS) + 2} features for parity\n")

    mira_results = []
    claude_results = []

    # Run standard feature tests
    for test in FEATURE_TESTS:
        mira, claude = run_feature_test(test)
        mira_results.append(mira)
        claude_results.append(claude)
        time.sleep(2)  # Delay between tests

    # Run special file operation tests
    mira, claude = test_file_write()
    mira_results.append(mira)
    claude_results.append(claude)
    time.sleep(2)

    mira, claude = test_file_edit()
    mira_results.append(mira)
    claude_results.append(claude)

    # Print and save results
    print_results(mira_results, claude_results)
    save_results(mira_results, claude_results)


if __name__ == "__main__":
    main()
