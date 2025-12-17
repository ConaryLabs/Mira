#!/usr/bin/env python3
"""
Benchmark comparison between mira-chat and Claude Code (with Mira MCP)

Measures:
- Response time (total wall clock)
- Token usage (input/output)
- Tool calls made
- Quality of output (manual review + automated checks)
"""

import json
import subprocess
import time
import re
import os
import sys
from dataclasses import dataclass, field, asdict
from typing import Optional
from datetime import datetime

# Configuration
MIRA_CHAT_BIN = "/home/peter/Mira/target/release/mira-chat"
CLAUDE_CODE_BIN = "claude"  # Assumes claude is in PATH
PROJECT_DIR = "/home/peter/Mira"
TASKS_FILE = "/home/peter/Mira/benchmarks/tasks.json"
RESULTS_DIR = "/home/peter/Mira/benchmarks/results"

@dataclass
class BenchmarkResult:
    task_id: str = ""
    system: str = ""  # "mira-chat" or "claude-code"
    start_time: str = ""
    duration_seconds: float = 0.0
    input_tokens: int = 0
    output_tokens: int = 0
    cached_tokens: int = 0
    reasoning_tokens: int = 0
    tool_calls: int = 0
    tool_names: list = field(default_factory=list)
    output: str = ""
    success: bool = False
    error: Optional[str] = None

    @property
    def total_tokens(self) -> int:
        return self.input_tokens + self.output_tokens

    @property
    def cache_hit_rate(self) -> float:
        if self.input_tokens == 0:
            return 0.0
        return (self.cached_tokens / self.input_tokens) * 100


def load_tasks() -> list:
    """Load benchmark tasks from JSON file"""
    with open(TASKS_FILE) as f:
        data = json.load(f)
    return data["tasks"]


def run_mira_chat(prompt: str, timeout: int = 120) -> BenchmarkResult:
    """Run a prompt through mira-chat and capture metrics"""
    result = BenchmarkResult(
        task_id="",
        system="mira-chat",
        start_time=datetime.now().isoformat()
    )

    start = time.time()

    try:
        # Build command with environment
        env = os.environ.copy()
        env["DATABASE_URL"] = "sqlite:///home/peter/Mira/data/mira.db"
        env["QDRANT_URL"] = "http://localhost:6334"

        # Load OpenAI key from .env or environment
        if "OPENAI_API_KEY" not in env:
            env_file = os.path.join(PROJECT_DIR, ".env")
            if os.path.exists(env_file):
                with open(env_file) as f:
                    for line in f:
                        if line.startswith("OPENAI_API_KEY="):
                            env["OPENAI_API_KEY"] = line.split("=", 1)[1].strip().strip('"')

        # Run mira-chat with prompt piped in
        proc = subprocess.run(
            [MIRA_CHAT_BIN, "-p", PROJECT_DIR],
            input=f"{prompt}\n/quit\n",
            capture_output=True,
            text=True,
            timeout=timeout,
            env=env,
            cwd=PROJECT_DIR
        )

        result.output = proc.stdout
        result.duration_seconds = time.time() - start

        # Parse token usage from output
        # Format: [tokens: X in / Y out, Z% cached]
        token_match = re.search(
            r'\[tokens:\s*(\d+)\s*in\s*/\s*(\d+)\s*out(?:\s*\((\d+)\s*reasoning\))?,\s*([\d.]+)%\s*cached\]',
            result.output
        )
        if token_match:
            result.input_tokens = int(token_match.group(1))
            result.output_tokens = int(token_match.group(2))
            if token_match.group(3):
                result.reasoning_tokens = int(token_match.group(3))
            cache_pct = float(token_match.group(4))
            result.cached_tokens = int(result.input_tokens * cache_pct / 100)

        # Count tool calls
        tool_matches = re.findall(r'\[tool:\s*(\w+)\]', result.output)
        result.tool_calls = len(tool_matches)
        result.tool_names = list(set(tool_matches))

        # Check for errors
        if proc.returncode != 0:
            result.error = proc.stderr or f"Exit code {proc.returncode}"
        else:
            result.success = True

    except subprocess.TimeoutExpired:
        result.duration_seconds = timeout
        result.error = f"Timeout after {timeout}s"
    except Exception as e:
        result.duration_seconds = time.time() - start
        result.error = str(e)

    return result


def run_claude_code(prompt: str, timeout: int = 120) -> BenchmarkResult:
    """Run a prompt through Claude Code and capture metrics"""
    result = BenchmarkResult(
        task_id="",
        system="claude-code",
        start_time=datetime.now().isoformat()
    )

    start = time.time()

    try:
        # Run claude with prompt
        # Use --print to get output without interactive mode
        proc = subprocess.run(
            [CLAUDE_CODE_BIN, "--print", prompt],
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=PROJECT_DIR
        )

        result.output = proc.stdout
        result.duration_seconds = time.time() - start

        # Claude Code shows token usage at the end in cost format
        # We'll need to parse differently or use the API stats
        # For now, estimate from output length
        result.output_tokens = len(result.output.split())  # Rough word count

        # Count tool mentions (Claude Code shows tool usage in output)
        tool_patterns = [
            r'Read\s+\w+',
            r'Grep\s+',
            r'Glob\s+',
            r'Edit\s+',
            r'Write\s+',
            r'Bash\s+',
        ]
        for pattern in tool_patterns:
            matches = re.findall(pattern, result.output, re.IGNORECASE)
            result.tool_calls += len(matches)

        if proc.returncode != 0:
            result.error = proc.stderr or f"Exit code {proc.returncode}"
        else:
            result.success = True

    except subprocess.TimeoutExpired:
        result.duration_seconds = timeout
        result.error = f"Timeout after {timeout}s"
    except FileNotFoundError:
        result.duration_seconds = 0
        result.error = f"Claude Code not found at {CLAUDE_CODE_BIN}"
    except Exception as e:
        result.duration_seconds = time.time() - start
        result.error = str(e)

    return result


def verify_result(task: dict, result: BenchmarkResult) -> dict:
    """Verify that the task was completed correctly"""
    verification = {
        "passed": False,
        "checks": []
    }

    output_lower = result.output.lower()

    # Check expected files mentioned
    if "expected_files" in task:
        for f in task["expected_files"]:
            found = f.lower() in output_lower or f.split("/")[-1].lower() in output_lower
            verification["checks"].append({
                "type": "file_mentioned",
                "file": f,
                "passed": found
            })

    # Check expected keywords
    if "expected_keywords" in task:
        for kw in task["expected_keywords"]:
            found = kw.lower() in output_lower
            verification["checks"].append({
                "type": "keyword_found",
                "keyword": kw,
                "passed": found
            })

    # Check file edits
    if "verify_file" in task and "verify_pattern" in task:
        try:
            with open(os.path.join(PROJECT_DIR, task["verify_file"])) as f:
                content = f.read()
            found = bool(re.search(task["verify_pattern"], content))
            verification["checks"].append({
                "type": "file_content",
                "pattern": task["verify_pattern"],
                "passed": found
            })
        except Exception as e:
            verification["checks"].append({
                "type": "file_content",
                "error": str(e),
                "passed": False
            })

    # Overall pass if all checks pass (or no checks)
    if verification["checks"]:
        verification["passed"] = all(c["passed"] for c in verification["checks"])
    else:
        verification["passed"] = result.success

    return verification


def print_comparison(mira_result: BenchmarkResult, claude_result: BenchmarkResult, task: dict):
    """Print side-by-side comparison"""
    print(f"\n{'='*70}")
    print(f"Task: {task['name']} ({task['id']})")
    print(f"Category: {task['category']}")
    print(f"{'='*70}")

    # Timing
    print(f"\n{'Metric':<25} {'mira-chat':>20} {'claude-code':>20}")
    print(f"{'-'*65}")
    print(f"{'Duration (s)':<25} {mira_result.duration_seconds:>20.2f} {claude_result.duration_seconds:>20.2f}")

    # Winner indicator
    if mira_result.duration_seconds < claude_result.duration_seconds:
        speedup = claude_result.duration_seconds / max(mira_result.duration_seconds, 0.01)
        print(f"{'  -> mira-chat faster by':<25} {speedup:>20.1f}x")
    elif claude_result.duration_seconds < mira_result.duration_seconds:
        speedup = mira_result.duration_seconds / max(claude_result.duration_seconds, 0.01)
        print(f"{'  -> claude-code faster by':<25} {speedup:>20.1f}x")

    # Tokens (mira-chat only has accurate data)
    print(f"{'Input tokens':<25} {mira_result.input_tokens:>20} {'N/A':>20}")
    print(f"{'Output tokens':<25} {mira_result.output_tokens:>20} {'~' + str(claude_result.output_tokens):>20}")
    print(f"{'Cached tokens':<25} {mira_result.cached_tokens:>20} {'N/A':>20}")
    print(f"{'Cache hit %':<25} {mira_result.cache_hit_rate:>19.1f}% {'N/A':>20}")
    print(f"{'Reasoning tokens':<25} {mira_result.reasoning_tokens:>20} {'N/A':>20}")

    # Tool usage
    print(f"{'Tool calls':<25} {mira_result.tool_calls:>20} {claude_result.tool_calls:>20}")
    print(f"{'Tools used':<25} {str(mira_result.tool_names)[:20]:>20} {'N/A':>20}")

    # Success
    mira_status = "✓ Success" if mira_result.success else f"✗ {mira_result.error}"
    claude_status = "✓ Success" if claude_result.success else f"✗ {claude_result.error}"
    print(f"{'Status':<25} {mira_status[:20]:>20} {claude_status[:20]:>20}")


def run_benchmark(tasks: list, systems: list = ["mira-chat", "claude-code"]) -> dict:
    """Run full benchmark suite"""
    results = {
        "timestamp": datetime.now().isoformat(),
        "tasks": []
    }

    for task in tasks:
        print(f"\n>>> Running task: {task['name']}")

        task_results = {
            "task": task,
            "results": {}
        }

        # Run on each system
        if "mira-chat" in systems:
            print("    Running mira-chat...")
            mira_result = run_mira_chat(task["prompt"])
            mira_result.task_id = task["id"]
            mira_verification = verify_result(task, mira_result)
            task_results["results"]["mira-chat"] = {
                "metrics": asdict(mira_result),
                "verification": mira_verification
            }

        if "claude-code" in systems:
            print("    Running claude-code...")
            claude_result = run_claude_code(task["prompt"])
            claude_result.task_id = task["id"]
            claude_verification = verify_result(task, claude_result)
            task_results["results"]["claude-code"] = {
                "metrics": asdict(claude_result),
                "verification": claude_verification
            }

        results["tasks"].append(task_results)

        # Print comparison if both ran
        if "mira-chat" in systems and "claude-code" in systems:
            print_comparison(mira_result, claude_result, task)

    return results


def print_summary(results: dict):
    """Print overall summary"""
    print(f"\n{'='*70}")
    print("BENCHMARK SUMMARY")
    print(f"{'='*70}")

    mira_times = []
    claude_times = []
    mira_tokens = []
    mira_success = 0
    claude_success = 0

    for task_result in results["tasks"]:
        if "mira-chat" in task_result["results"]:
            m = task_result["results"]["mira-chat"]["metrics"]
            mira_times.append(m["duration_seconds"])
            mira_tokens.append(m["input_tokens"] + m["output_tokens"])
            if m["success"]:
                mira_success += 1

        if "claude-code" in task_result["results"]:
            c = task_result["results"]["claude-code"]["metrics"]
            claude_times.append(c["duration_seconds"])
            if c["success"]:
                claude_success += 1

    n_tasks = len(results["tasks"])

    if mira_times:
        print(f"\nmira-chat:")
        print(f"  Total time: {sum(mira_times):.2f}s")
        print(f"  Avg time per task: {sum(mira_times)/len(mira_times):.2f}s")
        print(f"  Total tokens: {sum(mira_tokens)}")
        print(f"  Success rate: {mira_success}/{n_tasks} ({100*mira_success/n_tasks:.0f}%)")

    if claude_times:
        print(f"\nclaude-code:")
        print(f"  Total time: {sum(claude_times):.2f}s")
        print(f"  Avg time per task: {sum(claude_times)/len(claude_times):.2f}s")
        print(f"  Success rate: {claude_success}/{n_tasks} ({100*claude_success/n_tasks:.0f}%)")

    if mira_times and claude_times:
        speedup = sum(claude_times) / sum(mira_times) if sum(mira_times) > 0 else 0
        print(f"\nComparison:")
        if speedup > 1:
            print(f"  mira-chat is {speedup:.1f}x faster overall")
        elif speedup > 0:
            print(f"  claude-code is {1/speedup:.1f}x faster overall")


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Benchmark mira-chat vs Claude Code")
    parser.add_argument("--mira-only", action="store_true", help="Only run mira-chat")
    parser.add_argument("--claude-only", action="store_true", help="Only run claude-code")
    parser.add_argument("--task", type=str, help="Run specific task by ID")
    parser.add_argument("--save", action="store_true", help="Save results to JSON")
    args = parser.parse_args()

    # Determine which systems to run
    systems = ["mira-chat", "claude-code"]
    if args.mira_only:
        systems = ["mira-chat"]
    elif args.claude_only:
        systems = ["claude-code"]

    # Load tasks
    tasks = load_tasks()
    if args.task:
        tasks = [t for t in tasks if t["id"] == args.task]
        if not tasks:
            print(f"Task '{args.task}' not found")
            sys.exit(1)

    print(f"Running benchmark with {len(tasks)} tasks on {systems}")
    print(f"Project: {PROJECT_DIR}")

    # Run benchmark
    results = run_benchmark(tasks, systems)

    # Print summary
    print_summary(results)

    # Save results
    if args.save:
        os.makedirs(RESULTS_DIR, exist_ok=True)
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        output_file = os.path.join(RESULTS_DIR, f"benchmark_{timestamp}.json")
        with open(output_file, "w") as f:
            json.dump(results, f, indent=2, default=str)
        print(f"\nResults saved to: {output_file}")


if __name__ == "__main__":
    main()
