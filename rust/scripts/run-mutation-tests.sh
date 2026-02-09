#!/bin/bash
# run-mutation-tests.sh
#
# Comprehensive mutation testing script for ARM SMMU v3 implementation
#
# Usage:
#   ./scripts/run-mutation-tests.sh [--quick|--full|--module MODULE]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
MUTATION_SCORE_THRESHOLD=90
RESULTS_DIR="$PROJECT_ROOT/mutants.out"

# Parse command line arguments
MODE="full"
MODULE=""

while [[ $# -gt 0 ]]; do
    case $1 in
        --quick)
            MODE="quick"
            shift
            ;;
        --full)
            MODE="full"
            shift
            ;;
        --module)
            MODE="module"
            MODULE="$2"
            shift 2
            ;;
        --help)
            echo "Usage: $0 [--quick|--full|--module MODULE]"
            echo ""
            echo "Options:"
            echo "  --quick     Run mutation testing on sample of code (faster)"
            echo "  --full      Run comprehensive mutation testing (default)"
            echo "  --module    Run mutation testing on specific module"
            echo "  --help      Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            echo "Run with --help for usage information"
            exit 1
            ;;
    esac
done

echo -e "${BLUE}=== ARM SMMU v3 Mutation Testing ===${NC}"
echo ""

# Check if cargo-mutants is installed
if ! command -v cargo-mutants &> /dev/null; then
    echo -e "${RED}Error: cargo-mutants is not installed${NC}"
    echo ""
    echo "Install with:"
    echo "  cargo install cargo-mutants"
    exit 1
fi

echo -e "${GREEN}✓ cargo-mutants is installed${NC}"
echo ""

# Clean previous results
if [ -d "$RESULTS_DIR" ]; then
    echo -e "${YELLOW}Cleaning previous mutation test results...${NC}"
    rm -rf "$RESULTS_DIR"
fi

# Run mutation tests based on mode
echo -e "${BLUE}Running mutation tests in ${MODE} mode...${NC}"
echo ""

START_TIME=$(date +%s)

case $MODE in
    quick)
        echo "Running quick mutation test (sample of files)..."
        cargo mutants \
            --timeout 60 \
            --file smmu/src/types/pasid.rs \
            --file smmu/src/types/stream_id.rs \
            --file smmu/src/types/address.rs \
            --output json \
            2>&1 | tee mutation-test.log
        ;;
    module)
        if [ -z "$MODULE" ]; then
            echo -e "${RED}Error: --module requires a module path${NC}"
            exit 1
        fi
        echo "Running mutation test on module: $MODULE"
        cargo mutants \
            --timeout 300 \
            --file "$MODULE" \
            --output json \
            2>&1 | tee mutation-test.log
        ;;
    full)
        echo "Running comprehensive mutation testing..."
        echo "This may take a while (10-60 minutes depending on hardware)"
        echo ""
        cargo mutants \
            --timeout 300 \
            --output json \
            --jobs $(nproc) \
            2>&1 | tee mutation-test.log
        ;;
esac

END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))

echo ""
echo -e "${BLUE}=== Mutation Testing Complete ===${NC}"
echo "Duration: ${DURATION}s"
echo ""

# Check if results exist
if [ ! -f "$RESULTS_DIR/mutants.json" ]; then
    echo -e "${RED}Error: No results file found${NC}"
    echo "Expected: $RESULTS_DIR/mutants.json"
    exit 1
fi

# Parse and display results
echo -e "${BLUE}=== Mutation Testing Results ===${NC}"
echo ""

# Extract metrics using jq
if command -v jq &> /dev/null; then
    TOTAL=$(jq -r '.total_mutants // 0' "$RESULTS_DIR/mutants.json")
    KILLED=$(jq -r '.killed // 0' "$RESULTS_DIR/mutants.json")
    SURVIVED=$(jq -r '.survived // 0' "$RESULTS_DIR/mutants.json")
    TIMEOUT=$(jq -r '.timeout // 0' "$RESULTS_DIR/mutants.json")
    UNVIABLE=$(jq -r '.unviable // 0' "$RESULTS_DIR/mutants.json")

    # Calculate score
    if [ "$TOTAL" -gt 0 ]; then
        SCORE=$(echo "scale=2; ($KILLED / $TOTAL) * 100" | bc)
    else
        SCORE=0
    fi

    echo "Total Mutants:     $TOTAL"
    echo "Killed:            $KILLED"
    echo "Survived:          $SURVIVED"
    echo "Timeout:           $TIMEOUT"
    echo "Unviable:          $UNVIABLE"
    echo ""
    echo -e "${BLUE}Mutation Score:    ${SCORE}%${NC}"
    echo ""

    # Check threshold
    if (( $(echo "$SCORE >= $MUTATION_SCORE_THRESHOLD" | bc -l) )); then
        echo -e "${GREEN}✓ Mutation score ${SCORE}% meets ${MUTATION_SCORE_THRESHOLD}% threshold${NC}"
        PASS=0
    else
        echo -e "${RED}✗ Mutation score ${SCORE}% below ${MUTATION_SCORE_THRESHOLD}% threshold${NC}"
        PASS=1
    fi

    # Show surviving mutants if any
    if [ "$SURVIVED" -gt 0 ]; then
        echo ""
        echo -e "${YELLOW}=== Surviving Mutants ===${NC}"
        echo ""
        echo "The following mutants survived (not caught by tests):"
        echo ""
        cargo mutants --list --caught false || true
        echo ""
        echo "Review these mutants and add tests to catch them."
        echo "See MUTATION_TESTING.md for guidance."
    fi

    # Generate summary report
    echo ""
    echo -e "${BLUE}=== Generating Summary Report ===${NC}"
    echo ""

    REPORT_FILE="$PROJECT_ROOT/mutation-test-report.txt"

    cat > "$REPORT_FILE" <<EOF
ARM SMMU v3 Mutation Testing Report
Generated: $(date)
Duration: ${DURATION}s

Overall Metrics:
- Total Mutants:     $TOTAL
- Killed:            $KILLED
- Survived:          $SURVIVED
- Timeout:           $TIMEOUT
- Unviable:          $UNVIABLE
- Mutation Score:    ${SCORE}%

Status: $(if [ $PASS -eq 0 ]; then echo "PASS"; else echo "FAIL"; fi)
Threshold: ${MUTATION_SCORE_THRESHOLD}%

Results Location:
- JSON:  $RESULTS_DIR/mutants.json
- Logs:  $PROJECT_ROOT/mutation-test.log
- Report: $PROJECT_ROOT/mutation-test-report.txt

EOF

    if [ "$SURVIVED" -gt 0 ]; then
        echo "Surviving Mutants:" >> "$REPORT_FILE"
        cargo mutants --list --caught false >> "$REPORT_FILE" 2>&1 || true
    fi

    echo "Report saved to: $REPORT_FILE"

    # Show results location
    echo ""
    echo -e "${GREEN}Results available at:${NC}"
    echo "  JSON: $RESULTS_DIR/mutants.json"
    echo "  Report: $REPORT_FILE"
    echo ""

    exit $PASS
else
    echo -e "${YELLOW}Warning: jq not installed, cannot parse detailed results${NC}"
    echo "Install jq for detailed mutation testing metrics"
    echo ""
    echo "Results available at:"
    echo "  JSON: $RESULTS_DIR/mutants.json"
    exit 0
fi
