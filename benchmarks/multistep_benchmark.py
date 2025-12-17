#!/usr/bin/env python3
"""
Multi-step benchmark comparing Claude Code (with mira-mcp) vs mira-chat.

Tests both systems on identical multi-step tasks that require:
- Multiple tool calls in sequence
- File reading and understanding
- Code searching and navigation
- Code modifications

Measures: tokens, time, tool calls, cache hits, quality
"""

import subprocess
import json
import time
import os
import re
import sys
from dataclasses import dataclass, field
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
class StepResult:
    """Result from a single step"""
    step_name: str
    success: bool
    output: str
    tool_calls: int = 0
    input_tokens: int = 0
    output_tokens: int = 0
    cached_tokens: int = 0
    reasoning_tokens: int = 0
    duration_seconds: float = 0.0


@dataclass
class TaskResult:
    """Result from a complete multi-step task"""
    task_id: str
    task_name: str
    system: str  # "mira-chat" or "claude-code"
    steps: list = field(default_factory=list)
    total_input_tokens: int = 0
    total_output_tokens: int = 0
    total_cached_tokens: int = 0
    total_reasoning_tokens: int = 0
    total_tool_calls: int = 0
    total_duration: float = 0.0
    success: bool = False
    final_output: str = ""
    cost_usd: float = 0.0  # Actual cost (for Claude) or estimated (for mira-chat)


# Multi-step tasks that exercise the agentic loop
MULTISTEP_TASKS = [
    {
        "id": "investigate-and-fix",
        "name": "Investigate and Fix Bug Pattern",
        "description": "Find all uses of a pattern, understand the issue, and suggest a fix",
        "steps": [
            "Search the mira-chat codebase for any use of 'unwrap()' in error handling paths. List the files and line numbers.",
            "Read the most concerning file you found and explain why the unwrap() might be problematic there.",
            "Suggest a specific fix for that location, showing the before and after code."
        ]
    },
    {
        "id": "trace-data-flow",
        "name": "Trace Data Flow",
        "description": "Trace how data flows through the system",
        "steps": [
            "Find where the 'Usage' struct is defined in mira-chat.",
            "Trace how cached_tokens flows from the API response to being displayed to the user. List the files involved.",
            "Explain the complete flow in 3-4 sentences."
        ]
    },
    {
        "id": "refactor-analysis",
        "name": "Refactor Analysis",
        "description": "Analyze code and propose refactoring",
        "steps": [
            "Find the largest function in mira-chat/src/tools/ directory by line count.",
            "Read that function and identify what it does.",
            "Propose how it could be broken into smaller functions. Be specific about the split points."
        ]
    },
    {
        "id": "cross-file-understanding",
        "name": "Cross-File Understanding",
        "description": "Understand how multiple files work together",
        "steps": [
            "Find where ToolExecutor is defined and what methods it has.",
            "Find where ToolExecutor is used in the REPL module.",
            "Explain how tool execution works end-to-end in 2-3 sentences."
        ]
    },
]


def run_mira_chat_task(task: dict) -> TaskResult:
    """Run a multi-step task through mira-chat"""
    result = TaskResult(
        task_id=task["id"],
        task_name=task["name"],
        system="mira-chat"
    )

    # Build conversation with all steps
    conversation = []
    for i, step in enumerate(task["steps"]):
        conversation.append(f"Step {i+1}: {step}")

    full_prompt = f"""I need you to complete this multi-step task. Complete each step before moving to the next.

Task: {task['name']}
{task['description']}

{chr(10).join(conversation)}

Complete all steps in order, showing your work for each step."""

    # Run mira-chat with the full conversation
    start_time = time.time()

    env = os.environ.copy()
    env["DATABASE_URL"] = "sqlite:///home/peter/Mira/data/mira.db"

    try:
        # Clear session state before each task, then send prompt
        full_input = f"/clear\n{full_prompt}\n/quit\n"

        proc = subprocess.run(
            ["/home/peter/Mira/target/release/mira-chat", "-p", "/home/peter/Mira"],
            input=full_input,
            capture_output=True,
            text=True,
            timeout=180,
            env=env,
            cwd="/home/peter/Mira"
        )
        output = proc.stdout + proc.stderr
        result.final_output = output
        result.total_duration = time.time() - start_time

        # Parse token usage from output
        # Format: [tokens: X in / Y out (Z reasoning), W% cached]
        token_match = re.search(
            r'\[tokens:\s*(\d+)\s*in\s*/\s*(\d+)\s*out(?:\s*\((\d+)\s*reasoning\))?,\s*([\d.]+)%\s*cached\]',
            output
        )
        if token_match:
            result.total_input_tokens = int(token_match.group(1))
            result.total_output_tokens = int(token_match.group(2))
            if token_match.group(3):
                result.total_reasoning_tokens = int(token_match.group(3))
            cache_pct = float(token_match.group(4))
            result.total_cached_tokens = int(result.total_input_tokens * cache_pct / 100)

        # Count tool calls
        tool_calls = re.findall(r'\[tool:\s*(\w+)\]', output)
        result.total_tool_calls = len(tool_calls)

        # Check if task completed successfully (has substantive output)
        result.success = len(output) > 500 and "error" not in output.lower()[:200]

        # Estimate cost for mira-chat
        result.cost_usd = calculate_cost(
            result.total_input_tokens,
            result.total_output_tokens,
            result.total_cached_tokens,
            "mira-chat"
        )

        # Create step results based on output analysis
        for i, step in enumerate(task["steps"]):
            step_result = StepResult(
                step_name=f"Step {i+1}",
                success=True,  # Simplified - could parse more carefully
                output=f"Completed as part of full response",
                duration_seconds=result.total_duration / len(task["steps"])
            )
            result.steps.append(step_result)

    except subprocess.TimeoutExpired:
        result.final_output = "TIMEOUT"
        result.success = False
    except Exception as e:
        result.final_output = f"ERROR: {e}"
        result.success = False

    return result


def run_claude_code_task(task: dict) -> TaskResult:
    """Run a multi-step task through Claude Code"""
    result = TaskResult(
        task_id=task["id"],
        task_name=task["name"],
        system="claude-code"
    )

    # Build the same conversation
    conversation = []
    for i, step in enumerate(task["steps"]):
        conversation.append(f"Step {i+1}: {step}")

    full_prompt = f"""I need you to complete this multi-step task. Complete each step before moving to the next.

Task: {task['name']}
{task['description']}

{chr(10).join(conversation)}

Complete all steps in order, showing your work for each step."""

    start_time = time.time()

    try:
        proc = subprocess.run(
            ["claude", "-p", full_prompt, "--output-format", "json"],
            capture_output=True,
            text=True,
            timeout=180,
            cwd="/home/peter/Mira"
        )

        result.total_duration = time.time() - start_time

        # Parse JSON output
        try:
            data = json.loads(proc.stdout)
            result.final_output = data.get("result", proc.stdout)

            # Extract usage from Claude's format
            if "usage" in data:
                usage = data["usage"]
                # Total input = regular input + cache read + cache creation
                result.total_input_tokens = (
                    usage.get("input_tokens", 0) +
                    usage.get("cache_read_input_tokens", 0) +
                    usage.get("cache_creation_input_tokens", 0)
                )
                result.total_output_tokens = usage.get("output_tokens", 0)
                result.total_cached_tokens = usage.get("cache_read_input_tokens", 0)

            # Get cost directly from Claude
            if "total_cost_usd" in data:
                result.cost_usd = data["total_cost_usd"]

            # Count turns as proxy for tool calls
            result.total_tool_calls = data.get("num_turns", 1) - 1  # subtract 1 for initial prompt

            result.success = data.get("is_error", False) == False

        except json.JSONDecodeError:
            result.final_output = proc.stdout
            result.success = len(proc.stdout) > 200

        # Create step results
        for i, step in enumerate(task["steps"]):
            step_result = StepResult(
                step_name=f"Step {i+1}",
                success=True,
                output=f"Completed as part of full response",
                duration_seconds=result.total_duration / len(task["steps"])
            )
            result.steps.append(step_result)

    except subprocess.TimeoutExpired:
        result.final_output = "TIMEOUT"
        result.success = False
    except Exception as e:
        result.final_output = f"ERROR: {e}"
        result.success = False

    return result


def calculate_cost(input_tokens: int, output_tokens: int, cached_tokens: int, system: str) -> float:
    """Calculate estimated cost in dollars"""
    if system == "mira-chat":
        # GPT-5.2 pricing (estimated)
        input_cost = (input_tokens - cached_tokens) * 2.50 / 1_000_000
        cached_cost = cached_tokens * 0.625 / 1_000_000  # 75% discount
        output_cost = output_tokens * 10.00 / 1_000_000
    else:
        # Claude pricing
        input_cost = (input_tokens - cached_tokens) * 3.00 / 1_000_000
        cached_cost = cached_tokens * 0.30 / 1_000_000  # 90% discount
        output_cost = output_tokens * 15.00 / 1_000_000

    return input_cost + cached_cost + output_cost


def print_comparison(mira_results: list, claude_results: list):
    """Print a comparison of results"""
    print("\n" + "=" * 70)
    print("MULTI-STEP BENCHMARK COMPARISON")
    print("=" * 70)

    # Aggregate stats
    mira_totals = {
        "input": sum(r.total_input_tokens for r in mira_results),
        "output": sum(r.total_output_tokens for r in mira_results),
        "cached": sum(r.total_cached_tokens for r in mira_results),
        "reasoning": sum(r.total_reasoning_tokens for r in mira_results),
        "tools": sum(r.total_tool_calls for r in mira_results),
        "time": sum(r.total_duration for r in mira_results),
        "success": sum(1 for r in mira_results if r.success),
    }

    claude_totals = {
        "input": sum(r.total_input_tokens for r in claude_results),
        "output": sum(r.total_output_tokens for r in claude_results),
        "cached": sum(r.total_cached_tokens for r in claude_results),
        "tools": sum(r.total_tool_calls for r in claude_results),
        "time": sum(r.total_duration for r in claude_results),
        "success": sum(1 for r in claude_results if r.success),
    }

    # Calculate costs - use actual for Claude, estimated for mira-chat
    mira_cost = calculate_cost(mira_totals["input"], mira_totals["output"], mira_totals["cached"], "mira-chat")
    claude_cost = sum(r.cost_usd for r in claude_results)  # Use actual cost from Claude

    print(f"\n{'Metric':<25} {'mira-chat':>15} {'Claude Code':>15} {'Diff':>12}")
    print("-" * 70)

    print(f"{'Tasks Completed':<25} {mira_totals['success']}/{len(mira_results):>12} {claude_totals['success']}/{len(claude_results):>12}")
    print(f"{'Total Time (s)':<25} {mira_totals['time']:>15.1f} {claude_totals['time']:>15.1f} {mira_totals['time']/max(claude_totals['time'],1):.2f}x")
    print(f"{'Total Tool Calls':<25} {mira_totals['tools']:>15} {claude_totals['tools']:>15}")
    print(f"{'Input Tokens':<25} {mira_totals['input']:>15,} {claude_totals['input']:>15,}")
    print(f"{'Output Tokens':<25} {mira_totals['output']:>15,} {claude_totals['output']:>15,}")
    print(f"{'Cached Tokens':<25} {mira_totals['cached']:>15,} {claude_totals['cached']:>15,}")

    mira_cache_pct = (mira_totals['cached'] / max(mira_totals['input'], 1)) * 100
    claude_cache_pct = (claude_totals['cached'] / max(claude_totals['input'], 1)) * 100
    print(f"{'Cache Rate':<25} {mira_cache_pct:>14.1f}% {claude_cache_pct:>14.1f}%")

    if mira_totals['reasoning'] > 0:
        print(f"{'Reasoning Tokens':<25} {mira_totals['reasoning']:>15,} {'N/A':>15}")

    cost_ratio = mira_cost / claude_cost if claude_cost > 0 else 0
    print(f"{'Estimated Cost':<25} ${mira_cost:>14.4f} ${claude_cost:>14.4f} {cost_ratio:.2f}x")

    # Per-task breakdown
    print("\n" + "-" * 70)
    print("PER-TASK BREAKDOWN")
    print("-" * 70)

    for mira_r, claude_r in zip(mira_results, claude_results):
        print(f"\n{mira_r.task_name}")
        print(f"  mira-chat:   {mira_r.total_duration:5.1f}s, {mira_r.total_tool_calls:2} tools, {mira_r.total_input_tokens + mira_r.total_output_tokens:,} tokens {'✓' if mira_r.success else '✗'}")
        print(f"  Claude Code: {claude_r.total_duration:5.1f}s, {claude_r.total_tool_calls:2} tools, {claude_r.total_input_tokens + claude_r.total_output_tokens:,} tokens {'✓' if claude_r.success else '✗'}")


def save_results(mira_results: list, claude_results: list):
    """Save detailed results to JSON"""
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    results_dir = Path("/home/peter/Mira/benchmarks/results")
    results_dir.mkdir(exist_ok=True)

    data = {
        "timestamp": timestamp,
        "mira_chat": [
            {
                "task_id": r.task_id,
                "task_name": r.task_name,
                "success": r.success,
                "duration": r.total_duration,
                "input_tokens": r.total_input_tokens,
                "output_tokens": r.total_output_tokens,
                "cached_tokens": r.total_cached_tokens,
                "reasoning_tokens": r.total_reasoning_tokens,
                "tool_calls": r.total_tool_calls,
                "output_preview": r.final_output[:500] if r.final_output else ""
            }
            for r in mira_results
        ],
        "claude_code": [
            {
                "task_id": r.task_id,
                "task_name": r.task_name,
                "success": r.success,
                "duration": r.total_duration,
                "input_tokens": r.total_input_tokens,
                "output_tokens": r.total_output_tokens,
                "cached_tokens": r.total_cached_tokens,
                "tool_calls": r.total_tool_calls,
                "output_preview": r.final_output[:500] if r.final_output else ""
            }
            for r in claude_results
        ]
    }

    output_file = results_dir / f"multistep_{timestamp}.json"
    output_file.write_text(json.dumps(data, indent=2))
    print(f"\nDetailed results saved to: {output_file}")


def main():
    print("=" * 70)
    print("MULTI-STEP BENCHMARK: mira-chat vs Claude Code")
    print("=" * 70)
    print(f"Running {len(MULTISTEP_TASKS)} multi-step tasks through both systems\n")

    mira_results = []
    claude_results = []

    for task in MULTISTEP_TASKS:
        print(f"\n{'='*50}")
        print(f"Task: {task['name']}")
        print(f"{'='*50}")

        # Run through mira-chat
        print(f"\n[mira-chat] Running {len(task['steps'])} steps...")
        mira_result = run_mira_chat_task(task)
        mira_results.append(mira_result)
        print(f"  Done: {mira_result.total_duration:.1f}s, {mira_result.total_tool_calls} tools, {'✓' if mira_result.success else '✗'}")

        # Small delay between mira-chat and Claude to avoid any conflicts
        time.sleep(2)

        # Run through Claude Code
        print(f"\n[Claude Code] Running {len(task['steps'])} steps...")
        claude_result = run_claude_code_task(task)
        claude_results.append(claude_result)
        print(f"  Done: {claude_result.total_duration:.1f}s, {claude_result.total_tool_calls} tools, {'✓' if claude_result.success else '✗'}")

        # Small delay between tasks
        time.sleep(2)

    # Print comparison
    print_comparison(mira_results, claude_results)

    # Save results
    save_results(mira_results, claude_results)


if __name__ == "__main__":
    main()
