#!/usr/bin/env bash
set -euo pipefail

# build-single-binary.sh
# Main build script that creates the self-extracting airgapped binary
# Combines Rust binary + Docker images payload into single executable

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BUILD_DIR="${PROJECT_ROOT}/build"

# Parse arguments
FORCE_REFRESH=false
for arg in "$@"; do
    case $arg in
        --force-refresh)
            FORCE_REFRESH=true
            shift
            ;;
        *)
            # Unknown argument
            ;;
    esac
done

# Ensure cargo is in PATH (e.g. when run from WSL/non-interactive shell)
if ! command -v cargo &> /dev/null; then
    if [ -f "${HOME}/.cargo/env" ]; then
        set +u
        source "${HOME}/.cargo/env"
        set -u
    fi
fi
if ! command -v cargo &> /dev/null; then
    echo "cargo not found. Install Rust: https://rustup.rs" >&2
    exit 1
fi
PAYLOAD_FILE="${BUILD_DIR}/payload.tar.gz"
PAYLOAD_MARKER="__NQRUST_PAYLOAD__"

resolve_version() {
    if [ -n "${VERSION:-}" ]; then
        echo "${VERSION#v}"; return
    fi
    if git -C "${PROJECT_ROOT}" rev-parse --git-dir >/dev/null 2>&1; then
        local tag
        tag="$(git -C "${PROJECT_ROOT}" describe --tags --abbrev=0 2>/dev/null || true)"
        if [ -n "${tag}" ]; then
            echo "${tag#v}"; return
        fi
    fi
    awk -F\" '/^version[[:space:]]*=/ {print $2; exit}' "${PROJECT_ROOT}/Cargo.toml"
}

PKG_VERSION="$(resolve_version)"
PKG_ARCH="amd64"
OUTPUT_NAME="nqrust-analytics-airgapped-installer-${PKG_VERSION}-${PKG_ARCH}"

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_step() {
    echo -e "${BLUE}[STEP]${NC} $1"
}

# Banner
echo ""
echo "=========================================="
echo "  NQRust Analytics Airgapped Builder"
echo "=========================================="
echo ""

# Step 1: Build Rust binary
log_step "Step 1/5: Building Rust binary..."
cd "${PROJECT_ROOT}"

if cargo build --release; then
    BINARY_PATH="${PROJECT_ROOT}/target/release/nqrust-analytics"
    BINARY_SIZE=$(du -h "${BINARY_PATH}" | cut -f1)
    log_info "✓ Rust binary built successfully (${BINARY_SIZE})"
else
    log_error "Failed to build Rust binary"
    exit 1
fi
echo ""

# Step 2: Save Docker images
log_step "Step 2/5: Saving Docker images..."
if [ "${FORCE_REFRESH}" = true ]; then
    log_info "Force refresh enabled, rebuilding images..."
    rm -rf "${BUILD_DIR}/images"
    log_info "Running save-images.sh..."
    "${SCRIPT_DIR}/save-images.sh"
elif [ ! -d "${BUILD_DIR}/images" ] || [ ! -f "${BUILD_DIR}/images/manifest.json" ]; then
    log_info "Running save-images.sh..."
    "${SCRIPT_DIR}/save-images.sh"
else
    log_info "Images already saved, skipping..."
    log_warn "To rebuild images, delete ${BUILD_DIR}/images and run again"
    log_warn "Or use --force-refresh flag to force rebuild"
fi
echo ""

# Step 3: Build payload
log_step "Step 3/5: Building payload..."
if [ "${FORCE_REFRESH}" = true ]; then
    log_info "Force refresh enabled, rebuilding payload..."
    rm -f "${PAYLOAD_FILE}"
    log_info "Running build-payload.sh..."
    "${SCRIPT_DIR}/build-payload.sh"
elif [ ! -f "${PAYLOAD_FILE}" ]; then
    log_info "Running build-payload.sh..."
    "${SCRIPT_DIR}/build-payload.sh"
else
    log_info "Payload already exists, skipping..."
    log_warn "To rebuild payload, delete ${PAYLOAD_FILE} and run again"
fi

PAYLOAD_SIZE=$(du -h "${PAYLOAD_FILE}" | cut -f1)
log_info "Payload size: ${PAYLOAD_SIZE}"
echo ""

# Step 4: Create self-extracting binary
log_step "Step 4/5: Creating self-extracting binary..."

OUTPUT_FILE="${PROJECT_ROOT}/${OUTPUT_NAME}"

# Remove old output if exists
if [ -f "${OUTPUT_FILE}" ]; then
    log_warn "Removing existing airgapped binary..."
    rm -f "${OUTPUT_FILE}"
fi

log_info "Combining binary + marker + payload..."

# Combine: [binary] + [marker] + [payload]
cat "${BINARY_PATH}" > "${OUTPUT_FILE}"
echo -n "${PAYLOAD_MARKER}" >> "${OUTPUT_FILE}"
cat "${PAYLOAD_FILE}" >> "${OUTPUT_FILE}"

# Make executable
chmod +x "${OUTPUT_FILE}"

OUTPUT_SIZE=$(du -h "${OUTPUT_FILE}" | cut -f1)
log_info "✓ Self-extracting binary created (${OUTPUT_SIZE})"
echo ""

# Step 5: Generate checksums
log_step "Step 5/5: Generating checksums..."

if command -v sha256sum &> /dev/null; then
    CHECKSUM=$(sha256sum "${OUTPUT_FILE}" | cut -d' ' -f1)
    echo "${CHECKSUM}  ${OUTPUT_NAME}" > "${PROJECT_ROOT}/${OUTPUT_NAME}.sha256"
elif command -v shasum &> /dev/null; then
    CHECKSUM=$(shasum -a 256 "${OUTPUT_FILE}" | cut -d' ' -f1)
    echo "${CHECKSUM}  ${OUTPUT_NAME}" > "${PROJECT_ROOT}/${OUTPUT_NAME}.sha256"
else
    log_warn "No SHA256 tool found, skipping checksum"
    CHECKSUM="unavailable"
fi

log_info "✓ Checksum generated"
echo ""

# Summary
echo "=========================================="
echo "  Build Complete!"
echo "=========================================="
echo ""
log_info "Output file: ${OUTPUT_FILE}"
log_info "File size: ${OUTPUT_SIZE}"
log_info "SHA256: ${CHECKSUM}"
echo ""
log_info "Build breakdown:"
log_info "  - Rust binary: ${BINARY_SIZE}"
log_info "  - Payload: ${PAYLOAD_SIZE}"
log_info "  - Marker: 16 bytes"
log_info "  - Total: ${OUTPUT_SIZE}"
echo ""
log_info "Next steps:"
log_info "  1. Verify: sha256sum -c ${OUTPUT_NAME}.sha256"
log_info "  2. Transfer to airgapped machine (USB/SCP/etc)"
log_info "  3. Run: ./${OUTPUT_NAME} install"
echo ""
