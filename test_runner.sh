#!/bin/bash
# test_runner.sh - Helper script for running data flow tests

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Load environment from .env file
load_env() {
    local env_file="${1:-.env}"
    
    if [ -f "$env_file" ]; then
        echo -e "${BLUE}Loading environment from $env_file...${NC}"
        set -a
        source "$env_file"
        set +a
        echo -e "${GREEN}✓ Loaded environment from $env_file${NC}"
    else
        echo -e "${YELLOW}⚠ No $env_file file found${NC}"
    fi
}

# Check prerequisites
check_prerequisites() {
    echo -e "${BLUE}Checking prerequisites...${NC}"
    
    # Try to load from .env if not already set
    if [ -z "$OPENAI_API_KEY" ]; then
        load_env
    fi
    
    # Check for OPENAI_API_KEY (could be from .env now)
    if [ -z "$OPENAI_API_KEY" ]; then
        echo -e "${RED}✗ OPENAI_API_KEY not set${NC}"
        echo "  Add it to .env file or export it: export OPENAI_API_KEY=\"your-key-here\""
        exit 1
    fi
    echo -e "${GREEN}✓ OPENAI_API_KEY is set${NC}"
    
    # Check Qdrant connectivity (use 6333 for REST API health check)
    local qdrant_host=${QDRANT_URL:-http://localhost:6333}
    if ! curl -s ${qdrant_host}/health > /dev/null 2>&1; then
        echo -e "${YELLOW}⚠ Qdrant not reachable at ${qdrant_host}${NC}"
        echo "  Start Qdrant with: docker-compose up -d"
        echo "  Or set QDRANT_URL in your .env file"
        echo ""
        read -p "Continue anyway? (y/n) " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 1
        fi
    else
        echo -e "${GREEN}✓ Qdrant is reachable${NC}"
    fi
    
    # Check for cargo
    if ! command -v cargo &> /dev/null; then
        echo -e "${RED}✗ cargo not found${NC}"
        echo "  Install Rust from: https://rustup.rs"
        exit 1
    fi
    echo -e "${GREEN}✓ cargo is installed${NC}"
    
    echo ""
}

# Clean up Qdrant test collections
cleanup_qdrant() {
    echo -e "${BLUE}Cleaning up Qdrant test collections...${NC}"
    
    local qdrant_host=${QDRANT_URL:-http://localhost:6333}
    local collections=("test_collection" "test_search" "test_multihead" "test_deletion" "test_full_flow" "e2e_test" "test_ops" "test_artifacts" "test_code_embedding" "test_cleanup")
    
    for collection in "${collections[@]}"; do
        curl -s -X DELETE "${qdrant_host}/collections/${collection}" > /dev/null 2>&1
    done
    
    echo -e "${GREEN}✓ Cleanup complete${NC}\n"
}

# Run a specific test suite
run_test() {
    local test_name=$1
    local flags=$2
    
    echo -e "${BLUE}Running ${test_name}...${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    if cargo test --test "${test_name}" -- ${flags}; then
        echo -e "${GREEN}✓ ${test_name} PASSED${NC}\n"
        return 0
    else
        echo -e "${RED}✗ ${test_name} FAILED${NC}\n"
        return 1
    fi
}

# Show usage
show_usage() {
    cat << EOF
Usage: ./test_runner.sh [OPTIONS]

OPTIONS:
    all                 Run all tests (default)
    pipeline            Run message pipeline tests only
    storage             Run storage & embedding tests only
    e2e                 Run end-to-end integration tests only
    operations          Run operation engine tests only
    artifacts           Run artifact flow tests only
    phase5              Run phase 5 provider tests only
    phase6              Run phase 6 integration tests only
    phase7              Run phase 7 routing tests only
    deepseek            Run DeepSeek live API tests only
    code-embedding      Run code embedding and search tests only
    embedding-cleanup   Run embedding cleanup (orphan removal) tests only
    context-builder     Run context builder & prompt assembly tests only
    rolling-summary     Run rolling summary generation tests only
    websocket-conn      Run WebSocket connection tests only
    websocket-routing   Run WebSocket message routing tests only
    quick               Run quick smoke test (complete message flow)
    cleanup             Clean up Qdrant test collections
    list                List all available tests
    help                Show this help message

EXAMPLES:
    ./test_runner.sh                    # Run all tests
    ./test_runner.sh pipeline           # Run pipeline tests only
    ./test_runner.sh operations         # Run operation engine tests
    ./test_runner.sh artifacts          # Run artifact flow tests
    ./test_runner.sh deepseek           # Run DeepSeek live API tests
    ./test_runner.sh code-embedding     # Run code embedding tests
    ./test_runner.sh embedding-cleanup  # Run embedding cleanup tests
    ./test_runner.sh context-builder    # Run context builder tests
    ./test_runner.sh rolling-summary    # Run rolling summary tests
    ./test_runner.sh websocket-conn     # Run WebSocket connection tests
    ./test_runner.sh websocket-routing  # Run WebSocket routing tests
    ./test_runner.sh quick              # Quick smoke test
    ./test_runner.sh cleanup            # Clean up test data

FLAGS (set as environment variables):
    VERBOSE=1           Show detailed test output
    NO_CLEANUP=1        Skip cleanup before running tests
    THREADS=N           Set number of test threads (default: 1)

EOF
}

# List all available tests
list_tests() {
    echo -e "${BLUE}Available Tests:${NC}\n"
    
    echo -e "${YELLOW}Message Pipeline Tests (message_pipeline_flow_test):${NC}"
    cargo test --test message_pipeline_flow_test -- --list 2>/dev/null | grep "test_" | sed 's/^/  /' || echo "  (test file not found)"
    echo ""
    
    echo -e "${YELLOW}Storage & Embedding Tests (storage_embedding_flow_test):${NC}"
    cargo test --test storage_embedding_flow_test -- --list 2>/dev/null | grep "test_" | sed 's/^/  /' || echo "  (test file not found)"
    echo ""
    
    echo -e "${YELLOW}End-to-End Tests (e2e_data_flow_test):${NC}"
    cargo test --test e2e_data_flow_test -- --list 2>/dev/null | grep "test_" | sed 's/^/  /' || echo "  (test file not found)"
    echo ""
    
    echo -e "${YELLOW}Operation Engine Tests (operation_engine_test):${NC}"
    cargo test --test operation_engine_test -- --list 2>/dev/null | grep "test_" | sed 's/^/  /' || echo "  (test file not found)"
    echo ""
    
    echo -e "${YELLOW}Artifact Flow Tests (artifact_flow_test):${NC}"
    cargo test --test artifact_flow_test -- --list 2>/dev/null | grep "test_" | sed 's/^/  /' || echo "  (test file not found)"
    echo ""
    
    echo -e "${YELLOW}Phase 5 Provider Tests (phase5_providers_test):${NC}"
    cargo test --test phase5_providers_test -- --list 2>/dev/null | grep "test_" | sed 's/^/  /' || echo "  (test file not found)"
    echo ""
    
    echo -e "${YELLOW}Phase 6 Integration Tests (phase6_integration_test):${NC}"
    cargo test --test phase6_integration_test -- --list 2>/dev/null | grep "test_" | sed 's/^/  /' || echo "  (test file not found)"
    echo ""
    
    echo -e "${YELLOW}Phase 7 Routing Tests (phase7_routing_test):${NC}"
    cargo test --test phase7_routing_test -- --list 2>/dev/null | grep "test_" | sed 's/^/  /' || echo "  (test file not found)"
    echo ""
    
    echo -e "${YELLOW}DeepSeek Live Tests (deepseek_live_test):${NC}"
    cargo test --test deepseek_live_test -- --list 2>/dev/null | grep "test_" | sed 's/^/  /' || echo "  (test file not found)"
    echo ""
    
    echo -e "${YELLOW}Code Embedding Tests (code_embedding_test):${NC}"
    cargo test --test code_embedding_test -- --list 2>/dev/null | grep "test_" | sed 's/^/  /' || echo "  (test file not found)"
    echo ""
    
    echo -e "${YELLOW}Embedding Cleanup Tests (embedding_cleanup_test):${NC}"
    cargo test --test embedding_cleanup_test -- --list 2>/dev/null | grep "test_" | sed 's/^/  /' || echo "  (test file not found)"
    echo ""
    
    echo -e "${YELLOW}Context Builder & Prompt Assembly Tests (context_builder_prompt_assembly_test):${NC}"
    cargo test --test context_builder_prompt_assembly_test -- --list 2>/dev/null | grep "test_" | sed 's/^/  /' || echo "  (test file not found)"
    echo ""
    
    echo -e "${YELLOW}Rolling Summary Tests (rolling_summary_test):${NC}"
    cargo test --test rolling_summary_test -- --list 2>/dev/null | grep "test_" | sed 's/^/  /' || echo "  (test file not found)"
    echo ""
    
    echo -e "${YELLOW}WebSocket Connection Tests (websocket_connection_test):${NC}"
    cargo test --test websocket_connection_test -- --list 2>/dev/null | grep "test_" | sed 's/^/  /' || echo "  (test file not found)"
    echo ""
    
    echo -e "${YELLOW}WebSocket Message Routing Tests (websocket_message_routing_test):${NC}"
    cargo test --test websocket_message_routing_test -- --list 2>/dev/null | grep "test_" | sed 's/^/  /' || echo "  (test file not found)"
    echo ""
}

# Main execution
main() {
    local command=${1:-all}
    local test_flags="--nocapture"
    local threads=${THREADS:-1}
    
    # Add test threads flag
    test_flags="${test_flags} --test-threads=${threads}"
    
    # Add verbose flag if requested
    if [ ! -z "$VERBOSE" ]; then
        test_flags="${test_flags} --show-output"
    fi
    
    case $command in
        help|--help|-h)
            show_usage
            exit 0
            ;;
        
        list)
            list_tests
            exit 0
            ;;
        
        cleanup)
            cleanup_qdrant
            exit 0
            ;;
        
        pipeline)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            run_test "message_pipeline_flow_test" "$test_flags"
            exit $?
            ;;
        
        storage)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            run_test "storage_embedding_flow_test" "$test_flags"
            exit $?
            ;;
        
        e2e)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            run_test "e2e_data_flow_test" "$test_flags"
            exit $?
            ;;
        
        operations)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            run_test "operation_engine_test" "$test_flags"
            exit $?
            ;;
        
        artifacts)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            run_test "artifact_flow_test" "$test_flags"
            exit $?
            ;;
        
        phase5)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            run_test "phase5_providers_test" "$test_flags"
            exit $?
            ;;
        
        phase6)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            run_test "phase6_integration_test" "$test_flags"
            exit $?
            ;;
        
        phase7)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            run_test "phase7_routing_test" "$test_flags"
            exit $?
            ;;
        
        deepseek)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            run_test "deepseek_live_test" "$test_flags"
            exit $?
            ;;
        
        code-embedding)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            run_test "code_embedding_test" "$test_flags"
            exit $?
            ;;
        
        embedding-cleanup)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            run_test "embedding_cleanup_test" "$test_flags"
            exit $?
            ;;
        
        context-builder)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            run_test "context_builder_prompt_assembly_test" "$test_flags"
            exit $?
            ;;
        
        rolling-summary)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            run_test "rolling_summary_test" "$test_flags"
            exit $?
            ;;
        
        websocket-conn)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            run_test "websocket_connection_test" "$test_flags"
            exit $?
            ;;
        
        websocket-routing)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            run_test "websocket_message_routing_test" "$test_flags"
            exit $?
            ;;
        
        quick)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            echo -e "${BLUE}Running quick smoke test...${NC}"
            cargo test --test e2e_data_flow_test test_complete_message_flow -- $test_flags
            exit $?
            ;;
        
        all)
            check_prerequisites
            if [ -z "$NO_CLEANUP" ]; then
                cleanup_qdrant
            fi
            
            echo -e "${BLUE}╔════════════════════════════════════════════════════════╗${NC}"
            echo -e "${BLUE}║          FULL TEST SUITE                               ║${NC}"
            echo -e "${BLUE}╚════════════════════════════════════════════════════════╝${NC}\n"
            
            local failed=0
            
            # Run all test suites
            run_test "message_pipeline_flow_test" "$test_flags" || failed=$((failed + 1))
            run_test "storage_embedding_flow_test" "$test_flags" || failed=$((failed + 1))
            run_test "e2e_data_flow_test" "$test_flags" || failed=$((failed + 1))
            run_test "operation_engine_test" "$test_flags" || failed=$((failed + 1))
            run_test "artifact_flow_test" "$test_flags" || failed=$((failed + 1))
            run_test "phase5_providers_test" "$test_flags" || failed=$((failed + 1))
            run_test "phase6_integration_test" "$test_flags" || failed=$((failed + 1))
            run_test "phase7_routing_test" "$test_flags" || failed=$((failed + 1))
            run_test "deepseek_live_test" "$test_flags" || failed=$((failed + 1))
            run_test "code_embedding_test" "$test_flags" || failed=$((failed + 1))
            run_test "embedding_cleanup_test" "$test_flags" || failed=$((failed + 1))
            run_test "context_builder_prompt_assembly_test" "$test_flags" || failed=$((failed + 1))
            run_test "rolling_summary_test" "$test_flags" || failed=$((failed + 1))
            run_test "websocket_connection_test" "$test_flags" || failed=$((failed + 1))
            run_test "websocket_message_routing_test" "$test_flags" || failed=$((failed + 1))
            
            # Summary
            echo -e "${BLUE}╔════════════════════════════════════════════════════════╗${NC}"
            if [ $failed -eq 0 ]; then
                echo -e "${GREEN}║  ALL TESTS PASSED ✓                                    ║${NC}"
            else
                echo -e "${RED}║  ${failed} TEST SUITE(S) FAILED ✗                            ║${NC}"
            fi
            echo -e "${BLUE}╚════════════════════════════════════════════════════════╝${NC}\n"
            
            exit $failed
            ;;
        
        *)
            echo -e "${RED}Unknown command: $command${NC}\n"
            show_usage
            exit 1
            ;;
    esac
}

# Run main function
main "$@"
