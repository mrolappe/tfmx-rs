#!/bin/sh
# Fetch the TFMX test corpus from Modland into this directory.
#
# The modules are copyrighted works of their composers and are NOT part of this
# repository -- this script only records how to obtain them. Run it from anywhere:
#
#     sh testdata/fetch.sh
#
set -eu

dir=$(dirname "$0")
base="https://modland.com/pub/modules/TFMX"

# "<author>/<module name>" -- deliberately spread across eras and sub-variants so
# that both header layouts (fixed-address vs. $1D0 offset table) are exercised.
modules="
Chris Huelsbeck/turrican intro
Chris Huelsbeck/turrican outside
Chris Huelsbeck/turrican 2 level 1-desert
Chris Huelsbeck/turrican 2 level 3-flight
Chris Huelsbeck/turrican 3 level 1
Chris Huelsbeck/apidya (title)
Chris Huelsbeck/apidya (level 1)
Chris Huelsbeck/r-type
Chris Huelsbeck/x-out (title)
Jochen Hippel/turrican 2 title (st)
"

# Percent-encode the characters Modland's paths actually contain.
urlencode() {
    printf '%s' "$1" | sed -e 's/ /%20/g' -e 's/(/%28/g' -e 's/)/%29/g' -e 's/+/%2B/g'
}

echo "$modules" | while IFS= read -r entry; do
    [ -n "$entry" ] || continue
    author=${entry%%/*}
    name=${entry#*/}
    for part in mdat smpl; do
        out="$dir/$part.$name"
        if [ -s "$out" ]; then
            echo "  have  $part.$name"
            continue
        fi
        url="$base/$(urlencode "$author")/$part.$(urlencode "$name")"
        if curl -fsS --max-time 60 -o "$out" "$url"; then
            echo "  got   $part.$name"
        else
            rm -f "$out"
            echo "  FAIL  $part.$name  ($url)" >&2
        fi
    done
done

echo
echo "corpus: $(ls "$dir" | grep -c '^mdat\.') modules"
