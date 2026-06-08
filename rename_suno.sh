#!/bin/bash
# Fetches titles from Suno and renames with clean V1/V2 versioning
MUSIC_DIR="/Users/xmacbookm5/Music/suno_xirtus/notherwave"
TMPDIR=$(mktemp -d)
cd "$MUSIC_DIR"

echo "Step 1: Fetching titles from Suno..."
for f in *.mp3; do
    uuid="${f%.mp3}"
    [[ "$uuid" =~ ^[a-f0-9]{8}-[a-f0-9]{4} ]] || continue
    
    title=$(curl -sS "https://suno.com/song/$uuid" \
        -H "User-Agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36" \
        --connect-timeout 10 --max-time 30 2>/dev/null | \
        grep -oE 'og:title"[^"]*content="[^"]*"' | head -1 | \
        sed 's/.*content="//' | sed 's/"//' | sed 's/ | Suno$//')
    
    [ -z "$title" ] && title="$uuid"
    safe=$(echo "$title" | tr '/' '-' | tr ':' '-' | tr -d "'\"" | tr -d '\n')
    
    echo "$uuid|$safe" >> "$TMPDIR/titles.txt"
    echo "  $uuid -> $safe"
    sleep 0.2
done

echo ""
echo "Step 2: Renaming with version numbers..."

# Get unique titles and process each group
cut -d'|' -f2 "$TMPDIR/titles.txt" | sort -u > "$TMPDIR/uniq.txt"

while IFS= read -r title; do
    # Get all UUIDs for this title (preserving original order)
    uuids=$(grep -F "|${title}$" "$TMPDIR/titles.txt" | cut -d'|' -f1)
    count=$(echo "$uuids" | wc -l | tr -d ' ')
    
    v=1
    for uuid in $uuids; do
        old="${uuid}.mp3"
        if [ "$count" -eq 1 ]; then
            newname="${title}_Xirtus.mp3"
        else
            newname="${title}_V${v}_Xirtus.mp3"
        fi
        
        if [ -f "$old" ] && [ "$old" != "$newname" ]; then
            mv "$old" "$newname"
            echo "  $newname"
        elif [ -f "$newname" ]; then
            echo "  $newname (already exists, skipping $old)"
        fi
        v=$((v + 1))
    done
done < "$TMPDIR/uniq.txt"

rm -rf "$TMPDIR"
echo ""
echo "Done!"
ls -1 | sort
