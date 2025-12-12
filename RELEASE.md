# Release & Distribution Guide

Dokumen ini menjelaskan sistem rilis otomatis dan cara menggunakan binary pre-built untuk aplikasi installer-analytics.

## 🚀 Sistem Rilis Otomatis

Aplikasi ini menggunakan GitHub Actions untuk membuat release otomatis setiap kali Anda push tag versi (contoh: `v0.1.0`).

### Cara Membuat Release

1. **Buat dan push tag versi:**
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```

2. **GitHub Actions akan otomatis:**
   - Build binary untuk `x86_64-unknown-linux-musl` (static binary, tidak butuh dependensi OS)
   - Membuat GitHub Release dengan tag tersebut
   - Upload binary sebagai release asset
   - Generate checksum SHA256 untuk verifikasi

3. **Release akan tersedia di:**
   - `https://github.com/YOUR_USERNAME/installer-analytics/releases/tag/v0.1.0`

## 📦 Instalasi Binary Pre-built

### Metode 1: Menggunakan Install Script (Recommended)

Script `install.sh` akan otomatis mendeteksi versi terbaru, mendownload, dan menginstall binary.

#### Prasyarat: GitHub Token

Karena repository ini **private**, Anda perlu menyediakan GitHub Personal Access Token (PAT) atau Fine-grained Token untuk mengakses release.

#### Langkah-langkah:

1. **Buat GitHub Token:**

   **Opsi A: Personal Access Token (Classic)**
   - Buka: https://github.com/settings/tokens/new
   - Berikan nama token (contoh: "installer-analytics-download")
   - Pilih scope: `repo` (untuk akses ke private repository)
   - Klik "Generate token"
   - **Salin token** (hanya muncul sekali!)

   **Opsi B: Fine-grained Token (Recommended)**
   - Buka: https://github.com/settings/tokens?type=beta
   - Klik "Generate new token"
   - Pilih repository: `installer-analytics`
   - Berikan nama token
   - Repository permissions:
     - `Contents`: Read-only (untuk download release assets)
     - `Metadata`: Read-only (otomatis)
   - Klik "Generate token"
   - **Salin token**

2. **Set Environment Variable:**
   ```bash
   export GITHUB_TOKEN="ghp_your_token_here"
   ```

3. **Edit install.sh:**
   Buka `install.sh` dan ubah:
   ```bash
   OWNER="YOUR_GITHUB_USERNAME"  # Ganti dengan username/organisasi Anda
   ```

4. **Jalankan Install Script:**
   ```bash
   curl -fsSL https://raw.githubusercontent.com/YOUR_USERNAME/installer-analytics/main/install.sh | bash
   ```

   Atau jika sudah clone repository:
   ```bash
   chmod +x install.sh
   ./install.sh
   ```

### Metode 2: Download Manual

Jika Anda ingin mendownload secara manual:

1. **Dapatkan Asset URL dari API:**
   ```bash
   export GITHUB_TOKEN="your_token_here"
   export VERSION="v0.1.0"
   export OWNER="your_username"
   export REPO="installer-analytics"
   
   # Dapatkan asset ID
   ASSET_ID=$(curl -sSL \
     -H "Authorization: Bearer ${GITHUB_TOKEN}" \
     -H "Accept: application/vnd.github+json" \
     "https://api.github.com/repos/${OWNER}/${REPO}/releases/tags/${VERSION}" | \
     grep -oP '"id":\s*\K[0-9]+(?=.*"name":\s*".*\.tar\.gz")' | head -1)
   
   # Download binary
   curl -L \
     -H "Authorization: Bearer ${GITHUB_TOKEN}" \
     -H "Accept: application/octet-stream" \
     "https://api.github.com/repos/${OWNER}/${REPO}/releases/assets/${ASSET_ID}" \
     -o installer-analytics-${VERSION}.tar.gz
   ```

2. **Extract dan Install:**
   ```bash
   tar -xzf installer-analytics-${VERSION}.tar.gz
   chmod +x installer-analytics
   sudo mv installer-analytics /usr/local/bin/
   ```

## 🔐 Keamanan Token

### Best Practices:

1. **Jangan commit token ke repository:**
   - Gunakan environment variable
   - Jangan hardcode token di script

2. **Gunakan Fine-grained Token:**
   - Lebih aman karena scope terbatas
   - Bisa di-revoke per repository

3. **Set Expiration:**
   - Token sebaiknya memiliki expiration date
   - Update token secara berkala

4. **Untuk Production/CI:**
   - Gunakan GitHub Secrets untuk menyimpan token
   - Jangan expose token di logs

### Contoh Penggunaan Token di CI/CD:

Jika Anda ingin menggunakan token di CI/CD lain (bukan GitHub Actions):

```yaml
# .github/workflows/custom.yml
env:
  GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}  # Auto-provided oleh GitHub Actions
```

Untuk CI/CD eksternal (GitLab CI, Jenkins, dll):
```bash
# Set sebagai secret/environment variable
export GITHUB_TOKEN="your_token_here"
```

## 📋 Contoh Perintah Curl Final

Berikut adalah contoh perintah curl yang bisa diberikan ke user (dengan token):

```bash
# Set token terlebih dahulu
export GITHUB_TOKEN="ghp_your_token_here"

# Download dan install
curl -fsSL https://raw.githubusercontent.com/YOUR_USERNAME/installer-analytics/main/install.sh | bash
```

**Atau dalam satu baris (tidak recommended untuk production):**
```bash
GITHUB_TOKEN="ghp_your_token_here" curl -fsSL https://raw.githubusercontent.com/YOUR_USERNAME/installer-analytics/main/install.sh | bash
```

## 🔍 Verifikasi Binary

Setelah download, verifikasi checksum:

```bash
# Download checksum file
curl -L \
  -H "Authorization: Bearer ${GITHUB_TOKEN}" \
  "https://api.github.com/repos/${OWNER}/${REPO}/releases/assets/${CHECKSUM_ASSET_ID}" \
  -o installer-analytics-v0.1.0.tar.gz.sha256

# Verifikasi
sha256sum -c installer-analytics-v0.1.0.tar.gz.sha256
```

## 🛠️ Troubleshooting

### Error: "GITHUB_TOKEN tidak ditemukan"
- Pastikan environment variable `GITHUB_TOKEN` sudah di-set
- Cek dengan: `echo $GITHUB_TOKEN`

### Error: "Asset tidak ditemukan"
- Pastikan tag release sudah dibuat
- Pastikan token memiliki permission `repo` atau `Contents: Read`
- Cek apakah release sudah published (bukan draft)

### Error: "unauthorized" atau "403 Forbidden"
- Token mungkin expired atau tidak memiliki permission yang cukup
- Pastikan token memiliki akses ke repository private
- Untuk Fine-grained token, pastikan repository sudah di-assign

### Error: "OS tidak didukung"
- Saat ini hanya support Linux x86_64
- Untuk platform lain, build dari source: `cargo build --release`

## 📝 Catatan Penting

1. **Repository Private:**
   - Semua akses ke release memerlukan autentikasi
   - Token harus memiliki permission yang sesuai

2. **Binary Static:**
   - Binary dikompilasi dengan `x86_64-unknown-linux-musl`
   - Tidak memerlukan library sistem tambahan
   - Bisa dijalankan di distribusi Linux manapun

3. **Versioning:**
   - Gunakan semantic versioning: `v0.1.0`, `v1.0.0`, dll
   - Tag harus dimulai dengan `v` untuk trigger workflow

4. **Update:**
   - Script `install.sh` akan otomatis mendeteksi versi terbaru
   - Untuk update manual, jalankan script lagi

## 🔗 Referensi

- [GitHub Personal Access Tokens](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/creating-a-personal-access-token)
- [Fine-grained Personal Access Tokens](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/creating-a-personal-access-token#creating-a-fine-grained-personal-access-token)
- [GitHub Releases API](https://docs.github.com/en/rest/releases/releases)
- [GitHub Actions Secrets](https://docs.github.com/en/actions/security-guides/encrypted-secrets)
