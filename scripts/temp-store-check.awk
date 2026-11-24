# Gate rule (D-073, D-077): every Connection::open*( in non-test Rust code must call
# keep_temp_in_memory( within the next 25 lines of the same function, or carry a
# "// temp-store: <reason>" comment on its line or the line before. Prints "file:line" per miss.
# Usage: awk -f scripts/temp-store-check.awk file.rs ...   (POSIX awk; twin logic in check.ps1)
function flush(   i, j, ok, depth, seen, t, opens, closes) {
    for (i = 1; i <= n; i++) skipl[i] = 0
    for (i = 1; i < n; i++) {
        if (L[i] !~ /^#\[cfg\(test\)\][[:space:]]*$/ || L[i + 1] !~ /^(pub(\([a-z]+\))? )?mod[[:space:]]/) continue
        skipl[i] = 1; skipl[i + 1] = 1
        if (L[i + 1] ~ /;[[:space:]]*$/) continue
        depth = 0; seen = 0
        for (j = i + 1; j <= n; j++) {
            skipl[j] = 1
            t = L[j]; opens = gsub(/\{/, "", t); closes = gsub(/\}/, "", t)
            depth += opens - closes
            if (opens > 0) seen = 1
            if (seen && depth <= 0) break
        }
    }
    for (i = 1; i <= n; i++) {
        if (skipl[i]) continue
        if (L[i] ~ /^[[:space:]]*\/\//) continue
        if (L[i] !~ /Connection::open[A-Za-z_]*\(/ || L[i] ~ /Connection::open_in_memory/) continue
        if (L[i] ~ /\/\/ temp-store: / || (i > 1 && L[i - 1] ~ /\/\/ temp-store: /)) continue
        ok = 0
        for (j = i; j <= i + 25 && j <= n; j++) {
            if (j > i && L[j] ~ /^[[:space:]]*(pub(\([a-z]+\))? )?(async )?(unsafe )?fn[[:space:]]/) break
            if (L[j] ~ /keep_temp_in_memory\(/) { ok = 1; break }
        }
        if (!ok) print file ":" i
    }
}
FNR == 1 { if (file != "") flush(); file = FILENAME; n = 0 }
{ L[++n] = $0 }
END { if (file != "") flush() }
