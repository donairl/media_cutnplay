#!/usr/bin/env bash
set -euo pipefail

# Script untuk membuat release Linux (tarball & GitHub release) untuk Media CutNPlay

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

# 1. Ambil versi dari Cargo.toml
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d '"' -f2)
ARCH=$(uname -m)
APP_NAME="media_cutnplay"
TAG="v${VERSION}"
RELEASE_NAME="${APP_NAME}-${TAG}-linux-${ARCH}"
DIST_DIR="${REPO_ROOT}/dist"
PKG_DIR="${DIST_DIR}/${RELEASE_NAME}"
ARCHIVE="${DIST_DIR}/${RELEASE_NAME}.tar.gz"

echo "=============================================="
echo " Packaging Release: ${RELEASE_NAME}"
echo "=============================================="

# 2. Build release binary
echo "==> Mengompilasi binary release..."
cargo build --release

# 3. Siapkan folder packaging
echo "==> Menyiapkan folder paket..."
rm -rf "${PKG_DIR}"
mkdir -p "${PKG_DIR}" "${DIST_DIR}"

# Salin binary (font ikon sudah tertanam di dalamnya, jadi assets/ tidak ikut)
# Honor CARGO_TARGET_DIR (e.g. a build dir off an exFAT drive).
cp "${CARGO_TARGET_DIR:-${REPO_ROOT}/target}/release/${APP_NAME}" "${PKG_DIR}/"

# Salin README dan LICENSE jika ada
for doc in README.md LICENSE; do
    if [ -f "${REPO_ROOT}/${doc}" ]; then
        cp "${REPO_ROOT}/${doc}" "${PKG_DIR}/"
    fi
done

# Buat file desktop entry installer opsional
cat <<EOF > "${PKG_DIR}/media-cutnplay.desktop"
[Desktop Entry]
Type=Application
Name=Media CutNPlay
Comment=Pemutar, pemotong, dan transcoder media offline
Exec=${APP_NAME} %F
Terminal=false
Categories=AudioVideo;Video;AudioVideoEditing;
MimeType=video/mp4;video/x-matroska;video/webm;video/quicktime;video/mp2t;
EOF

# 4. Buat tar.gz archive
echo "==> Mengompres arsip ${ARCHIVE}..."
tar -C "${DIST_DIR}" -czf "${ARCHIVE}" "${RELEASE_NAME}"
rm -rf "${PKG_DIR}"

# 5. Buat SHA256 checksum
CHECKSUM_FILE="${ARCHIVE}.sha256"
(cd "${DIST_DIR}" && sha256sum "$(basename "${ARCHIVE}")" > "$(basename "${CHECKSUM_FILE}")")

echo ""
echo "Arsip release berhasil dibuat:"
echo "  - Arsip   : ${ARCHIVE}"
echo "  - Checksum: ${CHECKSUM_FILE}"
echo ""

# Tag dan upload butuh remote "origin"
if ! ORIGIN_URL=$(git remote get-url origin 2>/dev/null); then
    echo "Remote 'origin' belum diatur: tag dan upload GitHub dilewati."
    echo "Atur dulu dengan: git remote add origin <url>, lalu jalankan ulang script ini."
    exit 0
fi

# 6. Pembuatan Tag Git (opsional jika belum ada)
if ! git rev-parse "${TAG}" >/dev/null 2>&1; then
    read -rp "Tag ${TAG} belum ada. Buat git tag lokal dan push ke remote? (y/n): " CREATE_TAG
    if [ "${CREATE_TAG}" = "y" ] || [ "${CREATE_TAG}" = "Y" ]; then
        git tag -a "${TAG}" -m "Release ${TAG}"
        git push origin "${TAG}"
        echo "Tag ${TAG} berhasil dibuat dan di-push ke GitHub."
    fi
else
    echo "Tag ${TAG} sudah ada."
fi

# 7. Upload ke GitHub Release (gh CLI atau instruksi manual)
if command -v gh >/dev/null 2>&1; then
    read -rp "Upload release ke GitHub sekarang menggunakan gh CLI? (y/n): " UPLOAD_GH
    if [ "${UPLOAD_GH}" = "y" ] || [ "${UPLOAD_GH}" = "Y" ]; then
        echo "Mengunggah release ke GitHub..."
        gh release create "${TAG}" "${ARCHIVE}" "${CHECKSUM_FILE}" \
            --title "Release ${TAG}" \
            --notes "Linux release ${TAG} (${ARCH}). Butuh library FFmpeg terpasang di sistem."
        echo "Release berhasil diunggah ke GitHub!"
    fi
else
    # git@github.com:user/repo.git -> https://github.com/user/repo
    WEB_URL=$(echo "${ORIGIN_URL}" | sed -E 's#^git@([^:]+):#https://\1/#; s#\.git$##')
    echo "--------------------------------------------------------"
    echo "Tips: 'gh' CLI tidak ditemukan."
    echo "Anda dapat membuat release dan mengunggah file secara manual di:"
    echo "  ${WEB_URL}/releases/new?tag=${TAG}"
    echo "File yang perlu diunggah:"
    echo "  - ${ARCHIVE}"
    echo "  - ${CHECKSUM_FILE}"
    echo "--------------------------------------------------------"
fi
