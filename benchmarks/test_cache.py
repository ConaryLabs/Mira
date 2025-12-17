#!/usr/bin/env python3
"""
Test cache hit rates by running multiple queries in a single mira-chat session.

This sends several queries sequentially to the same process to verify:
1. LLM prompt caching (OpenAI cached_tokens)
2. File content caching (our FileCache)
"""

import subprocess
import os
import re
import time

PROJECT_DIR = "/home/peter/Mira"
MIRA_CHAT_BIN = "/home/peter/Mira/target/release/mira-chat"

# Queries to run in sequence - designed to test caching
QUERIES = [
    # Query 1: Read a file (cold)
    "Read the file mira-chat/src/tools/mod.rs and tell me how many lines it has.",

    # Query 2: Read same file again (should hit file cache)
    "Read mira-chat/src/tools/mod.rs again and list the public functions.",

    # Query 3: Related query (should hit LLM prompt cache)
    "What does the ToolExecutor struct in that file do?",

    # Query 4: Search (uses grep)
    "Search for 'FileCache' in the mira-chat crate.",

    # Query 5: Same search (should be faster with caching)
    "Search for 'FileCache' again and show me more context.",

    # Query 6: Different file
    "Read mira-chat/src/tools/file.rs and explain the caching mechanism.",

    # Query 7: Back to first file (should still be cached)
    "Go back to mod.rs and show me the execute function.",
]

def run_session_test():
    """Run multiple queries in a single session"""

    # Build input with all queries
    input_text = "\n".join(QUERIES) + "\n/quit\n"

    # Set up environment
    env = os.environ.copy()
    env["DATABASE_URL"] = "sqlite:///home/peter/Mira/data/mira.db"
    env["QDRANT_URL"] = "http://localhost:6334"

    # Load OpenAI key
    env_file = os.path.join(PROJECT_DIR, ".env")
    if os.path.exists(env_file):
        with open(env_file) as f:
            for line in f:
                if line.startswith("OPENAI_API_KEY="):
                    env["OPENAI_API_KEY"] = line.split("=", 1)[1].strip().strip('"')

    print("=" * 70)
    print("CACHE HIT TEST - Running {} queries in single session".format(len(QUERIES)))
    print("=" * 70)

    start = time.time()

    proc = subprocess.run(
        [MIRA_CHAT_BIN, "-p", PROJECT_DIR],
        input=input_text,
        capture_output=True,
        text=True,
        timeout=300,
        env=env,
        cwd=PROJECT_DIR
    )

    duration = time.time() - start
    output = proc.stdout

    # Parse all token usage lines
    # Format: [tokens: X in / Y out, Z% cached] or [tokens: X in / Y out (R reasoning), Z% cached]
    token_pattern = r'\[tokens:\s*(\d+)\s*in\s*/\s*(\d+)\s*out(?:\s*\((\d+)\s*reasoning\))?,\s*([\d.]+)%\s*cached\]'
    matches = re.findall(token_pattern, output)

    print(f"\nTotal duration: {duration:.2f}s")
    print(f"Queries: {len(QUERIES)}")
    print(f"Token reports found: {len(matches)}")
    print()

    if matches:
        print("Per-query token usage:")
        print("-" * 60)
        print(f"{'Query':<8} {'Input':>10} {'Output':>10} {'Cached':>10} {'Cache %':>10}")
        print("-" * 60)

        total_input = 0
        total_output = 0
        total_cached = 0

        for i, (inp, out, reasoning, cache_pct) in enumerate(matches, 1):
            inp = int(inp)
            out = int(out)
            cache_pct = float(cache_pct)
            cached = int(inp * cache_pct / 100)

            total_input += inp
            total_output += out
            total_cached += cached

            print(f"Q{i:<7} {inp:>10,} {out:>10,} {cached:>10,} {cache_pct:>9.1f}%")

        print("-" * 60)
        overall_cache_pct = (total_cached / total_input * 100) if total_input > 0 else 0
        print(f"{'TOTAL':<8} {total_input:>10,} {total_output:>10,} {total_cached:>10,} {overall_cache_pct:>9.1f}%")
        print()

        # Analysis
        print("ANALYSIS:")
        print("-" * 60)

        if len(matches) >= 2:
            first_cache = float(matches[0][3])
            later_caches = [float(m[3]) for m in matches[1:]]
            avg_later = sum(later_caches) / len(later_caches) if later_caches else 0

            print(f"First query cache rate: {first_cache:.1f}%")
            print(f"Subsequent queries avg: {avg_later:.1f}%")

            if avg_later > first_cache + 10:
                print("✓ CACHE WORKING: Later queries show higher cache rates")
            elif avg_later > 50:
                print("✓ CACHE WORKING: High cache rates observed")
            else:
                print("⚠ LOW CACHE RATES: May need investigation")

        # Cost estimate (GPT-4 pricing as rough estimate)
        # Input: $0.03/1K, Output: $0.06/1K, Cached: $0.015/1K
        uncached_input = total_input - total_cached
        cost_estimate = (uncached_input * 0.00003) + (total_cached * 0.000015) + (total_output * 0.00006)
        cost_without_cache = (total_input * 0.00003) + (total_output * 0.00006)
        savings = cost_without_cache - cost_estimate

        print()
        print(f"Estimated cost with cache: ${cost_estimate:.4f}")
        print(f"Cost without cache would be: ${cost_without_cache:.4f}")
        print(f"Savings from caching: ${savings:.4f} ({100*savings/cost_without_cache:.1f}%)")

    else:
        print("No token usage found in output!")
        print("\nOutput preview:")
        print(output[:2000])

    # Also check for tool calls
    tool_matches = re.findall(r'\[tool:\s*(\w+)\]', output)
    if tool_matches:
        print()
        print(f"Tool calls made: {len(tool_matches)}")
        from collections import Counter
        tool_counts = Counter(tool_matches)
        for tool, count in tool_counts.most_common():
            print(f"  {tool}: {count}")


if __name__ == "__main__":
    run_session_test()
