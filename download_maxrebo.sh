#!/bin/bash
# Download Max Rebo Band album from Suno playlist
# Playlist: https://suno.com/playlist/ea700df9-5ab8-44e6-a97f-7ad14a5d24af
set -e

OUTPUT_DIR="${1:-/Users/xmacbookm5/Music/suno_maxrebo}"
TMPDIR=$(mktemp -d)
mkdir -p "$OUTPUT_DIR"

URLS=(
"https://cdn1.suno.ai/00f2bbdd-066c-470a-af15-517c8e55a8b8.mp3"
"https://cdn1.suno.ai/0a9f5874-3604-4c27-b5d4-7e0a55cf4350.mp3"
"https://cdn1.suno.ai/0ac36503-7775-4948-ad6d-4239e50550d0.mp3"
"https://cdn1.suno.ai/137491a1-7008-4027-9fcb-6dbf8eb8d49e.mp3"
"https://cdn1.suno.ai/16f3a038-089e-4466-90a5-9ccedc154ed4.mp3"
"https://cdn1.suno.ai/16ff5baf-7619-44f3-b364-9d6e23382482.mp3"
"https://cdn1.suno.ai/18060897-5edf-4b9c-b738-9781df34fc7e.mp3"
"https://cdn1.suno.ai/19efb5d3-72d9-4f6f-a6ee-d0fe7d6aed4f.mp3"
"https://cdn1.suno.ai/1b22dec6-3978-4d7a-8570-8ec0d164b222.mp3"
"https://cdn1.suno.ai/22823ea5-ffea-4b79-9bb0-54eff0a787ab.mp3"
"https://cdn1.suno.ai/26139349-d7a8-42c2-9004-22cb6f9366c6.mp3"
"https://cdn1.suno.ai/35647c14-c5ba-4732-b304-51e200d56580.mp3"
"https://cdn1.suno.ai/4544f83b-4295-4cdd-acc3-3d540fd2dc9c.mp3"
"https://cdn1.suno.ai/487d664f-477b-4bf3-99f9-1ece0dd3b4bd.mp3"
"https://cdn1.suno.ai/51f91e72-79f1-4eeb-816c-d9ab7cbb7ae5.mp3"
"https://cdn1.suno.ai/52aa78dd-1205-4928-adba-f9add03e154a.mp3"
"https://cdn1.suno.ai/574127ce-aea2-444d-9b01-fc61e0eff29c.mp3"
"https://cdn1.suno.ai/57c2ecc5-d07a-42fd-8660-794db8bfe1c4.mp3"
"https://cdn1.suno.ai/5da5532f-4279-43db-b890-fa07ab9f59b7.mp3"
"https://cdn1.suno.ai/5f0a86b5-90fd-4e9c-bfe4-4ef4a901ebdd.mp3"
"https://cdn1.suno.ai/68ac3b2d-cb86-4388-ad71-bed376fc49dd.mp3"
"https://cdn1.suno.ai/7730f8db-a55c-405f-b492-176d461d753c.mp3"
"https://cdn1.suno.ai/77d95c43-b21b-43cb-9b59-666eb09d52e9.mp3"
"https://cdn1.suno.ai/78f28f67-3001-4479-89c7-d1f9de60d9ed.mp3"
"https://cdn1.suno.ai/85ecb3ac-5afc-4b76-a5ea-0fd199adf003.mp3"
"https://cdn1.suno.ai/87f5443f-d368-4f3f-8420-ff3763451e7b.mp3"
"https://cdn1.suno.ai/9077b03d-902e-43ff-8871-b6e4173571a0.mp3"
"https://cdn1.suno.ai/9336a17b-caa6-47e5-a24e-c8df0876beb8.mp3"
"https://cdn1.suno.ai/93efb12e-b1d7-411c-b34a-e158d89d6f88.mp3"
"https://cdn1.suno.ai/9554f7d3-b783-4a27-b84f-673e660109a3.mp3"
"https://cdn1.suno.ai/97cfc788-c042-43e8-a4b8-a0ade579f5ff.mp3"
"https://cdn1.suno.ai/97e4089a-c772-4082-9854-0c22ce8bab34.mp3"
"https://cdn1.suno.ai/a2e28513-f9d0-4bdd-91b5-aab67e34a895.mp3"
"https://cdn1.suno.ai/a7da114e-7ba9-4da0-9a2b-3ee82e00d1b3.mp3"
"https://cdn1.suno.ai/ad8820c8-7ef0-498e-919c-1739d83e8ae7.mp3"
"https://cdn1.suno.ai/b5acf1c8-3484-40b6-9736-79455de15c71.mp3"
"https://cdn1.suno.ai/c2c58787-6860-4baf-b63f-7102fd82ef03.mp3"
"https://cdn1.suno.ai/c5f37b4e-e126-4de8-87a0-80ef3f0957fa.mp3"
"https://cdn1.suno.ai/c9f0767b-291b-42dd-a6de-f5ef1326ff3b.mp3"
"https://cdn1.suno.ai/ce75100f-2119-4815-adc3-7759f5731240.mp3"
"https://cdn1.suno.ai/d09c2715-b777-4e17-83f4-644cd9c7ec20.mp3"
"https://cdn1.suno.ai/e07ba736-1ea1-48dd-b15c-2a283129f8d8.mp3"
"https://cdn1.suno.ai/e217c387-4ac7-4d6b-abfc-d20302e8a22d.mp3"
"https://cdn1.suno.ai/edcf83f5-152d-4a0e-a7aa-53191485f377.mp3"
"https://cdn1.suno.ai/f0393e86-c327-4848-ade6-bf8569461eb3.mp3"
"https://cdn1.suno.ai/f17151e2-87f4-422a-a83c-3876a23b9355.mp3"
"https://cdn1.suno.ai/f41f1196-19d7-4b92-bd43-243a9f3223b6.mp3"
"https://cdn1.suno.ai/f420aa57-8d23-417f-9ca6-5c84f91775e1.mp3"
"https://cdn1.suno.ai/f467fe7a-f0df-4f64-b3bb-9911e19d00b5.mp3"
"https://cdn1.suno.ai/fd2f7e05-49b6-43a6-9e45-d9b78d7f54ef.mp3"
)

UA="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36"

TOTAL=${#URLS[@]}
echo "🎵 Max Rebo Band - Downloading $TOTAL tracks"
echo "📁 Output: $OUTPUT_DIR"
echo ""

# Step 1: Download all MP3s
echo "━━━ Step 1: Downloading MP3s ━━━"
COUNT=0
FAILED=0
for url in "${URLS[@]}"; do
    COUNT=$((COUNT + 1))
    filename=$(basename "$url")
    if [ -f "$OUTPUT_DIR/$filename" ]; then
        printf "[%2d/%2d] %s ... (cached)\n" "$COUNT" "$TOTAL" "$filename"
    else
        printf "[%2d/%2d] %s ... " "$COUNT" "$TOTAL" "$filename"
        if curl -sS -L -o "$OUTPUT_DIR/$filename" "$url" --fail --connect-timeout 10 --max-time 120; then
            echo "✓"
        else
            echo "✗ FAILED"
            FAILED=$((FAILED + 1))
        fi
    fi
done

echo ""
echo "✅ Downloaded $((TOTAL - FAILED))/$TOTAL"
echo ""

# Step 2: Fetch titles from Suno
echo "━━━ Step 2: Fetching song titles ━━━"
COUNT=0
for f in "$OUTPUT_DIR"/*.mp3; do
    COUNT=$((COUNT + 1))
    uuid=$(basename "$f" .mp3)
    
    title=$(curl -sS "https://suno.com/song/$uuid" \
        -H "User-Agent: $UA" \
        --connect-timeout 10 --max-time 30 2>/dev/null | \
        grep -oE 'og:title"[^"]*content="[^"]*"' | head -1 | \
        sed 's/.*content="//' | sed 's/"//' | sed 's/ | Suno$//')
    
    [ -z "$title" ] && title="$uuid"
    
    # Clean filename: remove quotes, slashes, colons
    safe=$(echo "$title" | tr '/' '-' | tr ':' '-' | tr -d "'\"" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
    
    printf "[%2d/%2d] %s → %s\n" "$COUNT" "$TOTAL" "$uuid" "$safe"
    echo "$uuid|$safe" >> "$TMPDIR/titles.txt"
    sleep 0.2
done

echo ""

# Step 3: Rename with version numbers for duplicates
echo "━━━ Step 3: Renaming with versioning ━━━"
cd "$OUTPUT_DIR"

# Get unique titles (preserving first-seen order)
declare -A TITLE_ORDER
order=0
while IFS='|' read -r uuid title; do
    if [ -z "${TITLE_ORDER[$title]}" ]; then
        TITLE_ORDER[$title]=$order
        order=$((order + 1))
    fi
done < "$TMPDIR/titles.txt"

# Get titles sorted by first appearance
for title in "${!TITLE_ORDER[@]}"; do
    echo "${TITLE_ORDER[$title]} $title"
done | sort -n | cut -d' ' -f2- > "$TMPDIR/uniq_ordered.txt"

while IFS= read -r title; do
    uuids=$(grep -F "|${title}$" "$TMPDIR/titles.txt" | cut -d'|' -f1)
    count=$(echo "$uuids" | wc -l | tr -d ' ')
    
    v=1
    for uuid in $uuids; do
        old="${uuid}.mp3"
        if [ "$count" -eq 1 ]; then
            newname="${title}.mp3"
        else
            newname="${title}_V${v}.mp3"
        fi
        
        if [ -f "$old" ] && [ "$old" != "$newname" ]; then
            mv "$old" "$newname"
            echo "  ✓ $newname"
        elif [ -f "$newname" ]; then
            echo "  ✓ $newname (already exists)"
        else
            echo "  ✗ $old not found"
        fi
        v=$((v + 1))
    done
done < "$TMPDIR/uniq_ordered.txt"

rm -rf "$TMPDIR"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ Done! Final track listing:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
ls -1 "$OUTPUT_DIR" | sort
echo ""
echo "📁 All songs in: $OUTPUT_DIR"
