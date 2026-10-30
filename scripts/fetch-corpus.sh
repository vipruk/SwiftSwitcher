#!/usr/bin/env bash
# Downloads the full-corpus precision test data into target/corpus/.
#
# Source: Leipzig Corpora Collection, Universität Leipzig (https://wortschatz.uni-leipzig.de/)
#   eng_news_2020_10K and rus_news_2020_10K, licensed CC BY 4.0.
#   https://downloads.wortschatz-leipzig.de/corpora/eng_news_2020_10K.tar.gz
#   https://downloads.wortschatz-leipzig.de/corpora/rus_news_2020_10K.tar.gz
#
# The data is only downloaded for local testing; it is never committed.
# Result: target/corpus/eng-sentences.txt and target/corpus/rus-sentences.txt
# (one "id<TAB>sentence" per line).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/target/corpus"
base="https://downloads.wortschatz-leipzig.de/corpora"
mkdir -p "$out"

fetch() {
    local name="$1" lang="$2"
    local dest="$out/$lang-sentences.txt"
    if [[ -s "$dest" ]]; then
        echo "have $dest"
        return
    fi
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' RETURN
    curl -fsSL --retry 3 -o "$tmp/$name.tar.gz" "$base/$name.tar.gz"
    tar -xzf "$tmp/$name.tar.gz" -C "$tmp" --wildcards '*-sentences.txt'
    find "$tmp" -name '*-sentences.txt' -exec mv {} "$dest" \;
    [[ -s "$dest" ]] || { echo "no sentences file in $name" >&2; exit 1; }
    echo "wrote $dest"
}

fetch eng_news_2020_10K eng
fetch rus_news_2020_10K rus
