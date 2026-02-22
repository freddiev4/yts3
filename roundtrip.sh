#!/usr/bin/env bash
set -euo pipefail

INPUT="${1:-hello.txt}"
ENCODED="${2:-encoded.mkv}"
PASSWORD="${3:-password}"

echo "==> Hashing original: $INPUT"
ORIGINAL_HASH=$(shasum -a 256 "$INPUT" | awk '{print $1}')
echo "    $ORIGINAL_HASH"

echo "==> Encoding..."
yts3 encode --input "$INPUT" --output "$ENCODED" --password "$PASSWORD"

echo "==> Decoding..."
mv "$INPUT" "${INPUT}.bak"
yts3 decode --input "$ENCODED" --output "$INPUT" --password "$PASSWORD"

echo "==> Hashing decoded: $INPUT"
DECODED_HASH=$(shasum -a 256 "$INPUT" | awk '{print $1}')
echo "    $DECODED_HASH"

echo ""
if [ "$ORIGINAL_HASH" = "$DECODED_HASH" ]; then
    echo "PASS: hashes match"
    rm "${INPUT}.bak"
else
    echo "FAIL: hash mismatch!"
    echo "  original: $ORIGINAL_HASH"
    echo "  decoded:  $DECODED_HASH"
    mv "${INPUT}.bak" "${INPUT}.original"
    exit 1
fi
