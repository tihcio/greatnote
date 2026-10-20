/** Confronto per parole tra due testi, per l'anteprima delle modifiche AI. */

export type DiffPart = { kind: "same" | "added" | "removed"; text: string };

/** Oltre questa soglia (parole × parole) il confronto riga per riga
 *  diventerebbe troppo costoso: si mostrano i due testi interi. */
const MAX_CELLS = 4_000_000;

function tokenize(text: string): string[] {
  return text.match(/\s+|[^\s]+/g) ?? [];
}

function push(parts: DiffPart[], kind: DiffPart["kind"], text: string) {
  const last = parts[parts.length - 1];
  if (last && last.kind === kind) last.text += text;
  else parts.push({ kind, text });
}

export function diffWords(before: string, after: string): DiffPart[] {
  const a = tokenize(before);
  const b = tokenize(after);
  // Prefisso e suffisso comuni accorciano il problema.
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start += 1;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA -= 1;
    endB -= 1;
  }
  const parts: DiffPart[] = [];
  push(parts, "same", a.slice(0, start).join(""));
  const midA = a.slice(start, endA);
  const midB = b.slice(start, endB);
  if (midA.length * midB.length > MAX_CELLS) {
    push(parts, "removed", midA.join(""));
    push(parts, "added", midB.join(""));
  } else {
    // Sottosequenza comune più lunga (programmazione dinamica).
    const rows = midA.length + 1;
    const cols = midB.length + 1;
    const table = new Uint32Array(rows * cols);
    for (let i = midA.length - 1; i >= 0; i -= 1) {
      for (let j = midB.length - 1; j >= 0; j -= 1) {
        table[i * cols + j] =
          midA[i] === midB[j]
            ? table[(i + 1) * cols + j + 1] + 1
            : Math.max(table[(i + 1) * cols + j], table[i * cols + j + 1]);
      }
    }
    let i = 0;
    let j = 0;
    while (i < midA.length && j < midB.length) {
      if (midA[i] === midB[j]) {
        push(parts, "same", midA[i]);
        i += 1;
        j += 1;
      } else if (table[(i + 1) * cols + j] >= table[i * cols + j + 1]) {
        push(parts, "removed", midA[i]);
        i += 1;
      } else {
        push(parts, "added", midB[j]);
        j += 1;
      }
    }
    if (i < midA.length) push(parts, "removed", midA.slice(i).join(""));
    if (j < midB.length) push(parts, "added", midB.slice(j).join(""));
  }
  push(parts, "same", a.slice(endA).join(""));
  return parts.filter((part) => part.text.length > 0);
}
