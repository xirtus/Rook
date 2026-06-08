#!/bin/bash
# Download all songs from Suno playlist "notherwave by @xirtus"
# Playlist: https://suno.com/playlist/92066053-d514-4dc9-900f-c36e95498665

set -e

OUTPUT_DIR="${1:-./suno_notherwave}"
mkdir -p "$OUTPUT_DIR"

URLS=(
"https://cdn1.suno.ai/0039048f-2832-4912-b5f6-2aee0e14fb66.mp3"
"https://cdn1.suno.ai/0634ee6e-4d06-4594-bf98-cd84a8b6887c.mp3"
"https://cdn1.suno.ai/07254ee9-9570-47d0-a2f2-ff8e9e3a9671.mp3"
"https://cdn1.suno.ai/0844b4e7-9e69-4921-903c-b9d6749fe4f3.mp3"
"https://cdn1.suno.ai/0e82cde5-1297-4fdc-9a9e-8908a72145d3.mp3"
"https://cdn1.suno.ai/0eaab715-8ad0-45d3-8a6d-bf0518c1d423.mp3"
"https://cdn1.suno.ai/1794ee73-1e4a-4ed2-9759-632b45882667.mp3"
"https://cdn1.suno.ai/1b09c0fa-a4d5-4caa-8243-30eaf20ad70d.mp3"
"https://cdn1.suno.ai/1e257a18-32d0-4e97-a548-982aafe137e5.mp3"
"https://cdn1.suno.ai/23024ce3-9d9c-4a75-a745-082db8d5fa6b.mp3"
"https://cdn1.suno.ai/257cac49-2709-4304-8112-594397f10a91.mp3"
"https://cdn1.suno.ai/2b8c9e00-7b11-46c5-8a1f-a7d744abe9a3.mp3"
"https://cdn1.suno.ai/2dc5752f-6757-416e-9a10-541a74e490d9.mp3"
"https://cdn1.suno.ai/364926e0-eaf5-455a-a71f-edc921a9db80.mp3"
"https://cdn1.suno.ai/42449212-3eb4-4061-911b-831bebd4116e.mp3"
"https://cdn1.suno.ai/43c569ee-d767-4ca6-bd43-1f900e5878ca.mp3"
"https://cdn1.suno.ai/49fab3c4-37b4-4cb6-ae6e-b3c523a19859.mp3"
"https://cdn1.suno.ai/5a80aae6-f2bd-4948-b405-7f2dd6703d50.mp3"
"https://cdn1.suno.ai/5f56b7ab-9aaf-4d36-ab8e-a768f222a955.mp3"
"https://cdn1.suno.ai/687b5b5e-2299-41ab-9e5e-185afd5f1993.mp3"
"https://cdn1.suno.ai/6c75404e-3904-43a8-b3eb-03d366114305.mp3"
"https://cdn1.suno.ai/6d9a4b77-c8c4-4699-a4c4-0f8eb28092f0.mp3"
"https://cdn1.suno.ai/701fa74c-1f38-477c-b807-770412a7e208.mp3"
"https://cdn1.suno.ai/787ab825-28f8-4d5d-94a7-aee99ba9ef93.mp3"
"https://cdn1.suno.ai/7985bd69-2ffd-47b8-be64-94d69c327f0e.mp3"
"https://cdn1.suno.ai/7aa91677-a381-4351-9267-ad67e638b9f9.mp3"
"https://cdn1.suno.ai/7f97408c-724a-4be6-b845-629f8cd16846.mp3"
"https://cdn1.suno.ai/80c356a8-04b7-489e-b150-cda02886c4c3.mp3"
"https://cdn1.suno.ai/83b6d4d7-4df1-4e79-a310-9cf501ae2b98.mp3"
"https://cdn1.suno.ai/8aa3135a-7b9d-4506-9bc3-fc76c7a0a637.mp3"
"https://cdn1.suno.ai/8d76b8e1-da55-4c49-b07e-1eb6f98271e0.mp3"
"https://cdn1.suno.ai/8f412a11-c762-4e70-b338-6ab598811783.mp3"
"https://cdn1.suno.ai/9d596559-3b2d-4299-a021-e6587dd7f6ae.mp3"
"https://cdn1.suno.ai/a9a4b505-d9b6-410d-8dba-c8b7dfab1256.mp3"
"https://cdn1.suno.ai/ac8ee61e-46af-47fd-98a0-07c5e65ef5e2.mp3"
"https://cdn1.suno.ai/aedd224c-77a1-440e-b8aa-297dee0cfee3.mp3"
"https://cdn1.suno.ai/b1b03ef8-1702-4529-a871-4d17a780903b.mp3"
"https://cdn1.suno.ai/bdd4b54b-a59f-4ef7-8c7c-7a2dfe9b84d3.mp3"
"https://cdn1.suno.ai/bfd5f699-6859-40eb-850e-cb09dc27a4cd.mp3"
"https://cdn1.suno.ai/c56f222c-1400-445e-8f62-cd6f8ac1f0cb.mp3"
"https://cdn1.suno.ai/c649f37b-d8aa-4968-9556-84b3ae714d64.mp3"
"https://cdn1.suno.ai/c98fdbc5-c174-4c7d-a896-b8bf26bdf9c3.mp3"
"https://cdn1.suno.ai/da813582-45aa-4e54-83c8-31a2d1700ebd.mp3"
"https://cdn1.suno.ai/de5a01ea-303a-453c-aeb9-f8a260f8ec6b.mp3"
"https://cdn1.suno.ai/efca7b05-8cf7-4a31-98c9-1175f4f51f02.mp3"
"https://cdn1.suno.ai/f258a887-5d66-4a73-bf57-327368727527.mp3"
"https://cdn1.suno.ai/f3c3f69f-5ae6-4761-bf86-a880f01372a1.mp3"
"https://cdn1.suno.ai/f6a63249-9ef9-4765-a0f6-ee81d33b6b4a.mp3"
"https://cdn1.suno.ai/fabe3db3-c0d3-4600-a96c-e6413c5978a3.mp3"
"https://cdn1.suno.ai/fef20a13-a9d5-4130-95c3-859822b6c40d.mp3"
)

TOTAL=${#URLS[@]}
echo "🎵 Downloading $TOTAL songs from 'notherwave by @xirtus'"
echo "📁 Output: $OUTPUT_DIR"
echo ""

COUNT=0
FAILED=0
for url in "${URLS[@]}"; do
    COUNT=$((COUNT + 1))
    filename=$(basename "$url")
    printf "[%2d/%2d] %s ... " "$COUNT" "$TOTAL" "$filename"
    if curl -sS -L -o "$OUTPUT_DIR/$filename" "$url" --fail --connect-timeout 10 --max-time 120; then
        echo "✓"
    else
        echo "✗ FAILED"
        FAILED=$((FAILED + 1))
    fi
done

echo ""
echo "✅ Done! $((TOTAL - FAILED))/$TOTAL downloaded to: $OUTPUT_DIR"
ls -lh "$OUTPUT_DIR" | tail -n +2
