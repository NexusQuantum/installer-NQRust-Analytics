#!/bin/bash

# Installer script untuk installer-analytics
# Script ini akan mendownload dan menginstall binary dari GitHub Releases

set -euo pipefail

# Konstanta
REPO="installer-analytics"  # Ganti dengan nama repository Anda
OWNER="NexusQuantum"  # Ganti dengan username/organisasi GitHub Anda
BINARY_NAME="installer-analytics"
INSTALL_DIR="/usr/local/bin"
TEMP_DIR=$(mktemp -d)

# Colors untuk output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Fungsi untuk print pesan
info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
    exit 1
}

# Cleanup function
cleanup() {
    rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

# Deteksi OS dan Architecture
detect_platform() {
    local os=""
    local arch=""
    
    case "$(uname -s)" in
        Linux*)     os="linux" ;;
        Darwin*)    os="darwin" ;;
        *)          error "OS tidak didukung: $(uname -s)" ;;
    esac
    
    case "$(uname -m)" in
        x86_64)     arch="x86_64" ;;
        arm64|aarch64) arch="arm64" ;;
        *)          error "Architecture tidak didukung: $(uname -m)" ;;
    esac
    
    # Untuk saat ini, kita hanya support linux x86_64 dengan musl
    if [ "$os" != "linux" ] || [ "$arch" != "x86_64" ]; then
        error "Platform $os/$arch belum didukung. Saat ini hanya support linux/x86_64"
    fi
    
    echo "linux-x86_64-musl"
}

# Mendapatkan latest release version
get_latest_version() {
    local token="${GITHUB_TOKEN:-}"
    
    if [ -z "$token" ]; then
        error "GITHUB_TOKEN tidak ditemukan. Silakan set environment variable GITHUB_TOKEN dengan GitHub Personal Access Token atau Fine-grained Token."
    fi
    
    local api_url="https://api.github.com/repos/${OWNER}/${REPO}/releases/latest"
    
    local version=$(curl -sSL \
        -H "Accept: application/vnd.github+json" \
        -H "Authorization: Bearer ${token}" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        "$api_url" | grep -oP '"tag_name": "\K[^"]*' | head -1)
    
    if [ -z "$version" ]; then
        error "Gagal mendapatkan versi terbaru. Pastikan token memiliki akses ke repository."
    fi
    
    echo "$version"
}

# Download binary dari release
download_binary() {
    local version=$1
    local token="${GITHUB_TOKEN:-}"
    
    if [ -z "$token" ]; then
        error "GITHUB_TOKEN tidak ditemukan."
    fi
    
    local filename="${BINARY_NAME}-${version}-x86_64-unknown-linux-musl.tar.gz"
    local download_url="https://api.github.com/repos/${OWNER}/${REPO}/releases/assets"
    
    info "Mendownload release ${version}..."
    
    # Dapatkan asset ID dari API
    local asset_id=$(curl -sSL \
        -H "Accept: application/vnd.github+json" \
        -H "Authorization: Bearer ${token}" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        "https://api.github.com/repos/${OWNER}/${REPO}/releases/tags/${version}" | \
        grep -oP '"id":\s*\K[0-9]+(?=.*"name":\s*"'${filename}'")' | head -1)
    
    if [ -z "$asset_id" ]; then
        error "Asset ${filename} tidak ditemukan di release ${version}"
    fi
    
    # Download binary menggunakan asset ID
    curl -sSL \
        -H "Accept: application/octet-stream" \
        -H "Authorization: Bearer ${token}" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        -o "${TEMP_DIR}/${filename}" \
        "https://api.github.com/repos/${OWNER}/${REPO}/releases/assets/${asset_id}"
    
    if [ ! -f "${TEMP_DIR}/${filename}" ]; then
        error "Gagal mendownload binary"
    fi
    
    info "Binary berhasil didownload"
}

# Extract dan install binary
install_binary() {
    local version=$1
    local filename="${BINARY_NAME}-${version}-x86_64-unknown-linux-musl.tar.gz"
    
    info "Mengekstrak binary..."
    tar -xzf "${TEMP_DIR}/${filename}" -C "$TEMP_DIR"
    
    if [ ! -f "${TEMP_DIR}/${BINARY_NAME}" ]; then
        error "Binary tidak ditemukan setelah ekstraksi"
    fi
    
    # Berikan permission execute
    chmod +x "${TEMP_DIR}/${BINARY_NAME}"
    
    # Install ke /usr/local/bin (perlu sudo)
    info "Menginstall binary ke ${INSTALL_DIR}..."
    if [ -w "$INSTALL_DIR" ]; then
        cp "${TEMP_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
    else
        sudo cp "${TEMP_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
    fi
    
    info "Binary berhasil diinstall ke ${INSTALL_DIR}/${BINARY_NAME}"
}

# Verifikasi instalasi
verify_installation() {
    if command -v "$BINARY_NAME" &> /dev/null; then
        local installed_version=$($BINARY_NAME --version 2>/dev/null || echo "unknown")
        info "Instalasi berhasil!"
        info "Versi terinstall: $installed_version"
        info "Jalankan '${BINARY_NAME} --help' untuk melihat opsi yang tersedia"
    else
        warn "Binary terinstall tetapi tidak ditemukan di PATH. Pastikan ${INSTALL_DIR} ada di PATH Anda."
    fi
}

# Main function
main() {
    info "Installer untuk ${BINARY_NAME}"
    info "Repository: ${OWNER}/${REPO}"
    
    # Check jika sudah terinstall
    if command -v "$BINARY_NAME" &> /dev/null; then
        warn "${BINARY_NAME} sudah terinstall. Versi saat ini:"
        $BINARY_NAME --version 2>/dev/null || echo "unknown"
        read -p "Lanjutkan update? (y/N): " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            info "Instalasi dibatalkan"
            exit 0
        fi
    fi
    
    # Detect platform
    local platform=$(detect_platform)
    info "Platform terdeteksi: $platform"
    
    # Get latest version
    local version=$(get_latest_version)
    info "Versi terbaru: $version"
    
    # Download binary
    download_binary "$version"
    
    # Install binary
    install_binary "$version"
    
    # Verify installation
    verify_installation
}

# Run main
main
