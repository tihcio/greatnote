// Trascinamento di una nota dall'elenco verso la barra laterale (taccuini,
// spazi, tag, Scorciatoie, Archivio, Cestino).
//
// Usa gli eventi del puntatore e non il drag and drop HTML5: su Windows,
// con l'intercettazione dei file trascinati attiva nella finestra Tauri
// (necessaria per gli allegati), gli eventi HTML5 non arrivano alla pagina.
// Così il comportamento è lo stesso su Windows e Linux.
//
// Le destinazioni sono elementi con `data-drop="<tipo>:<id>"`; quella sotto
// il puntatore riceve `data-drop-active`. Esc annulla. L'alternativa da
// tastiera resta il menu della nota (Sposta in…, Modifica tag…).

/** Distanza in pixel oltre la quale la pressione diventa trascinamento. */
const THRESHOLD = 6;
/** Fascia vicino ai bordi della barra laterale che la fa scorrere. */
const EDGE = 48;

export interface DropTarget {
  kind: string;
  id: string;
}

interface DragOptions {
  label: string;
  onDrop: (target: DropTarget) => void;
}

function parseTarget(value: string | undefined): DropTarget | null {
  if (!value) return null;
  const separator = value.indexOf(":");
  if (separator < 0) return null;
  return { kind: value.slice(0, separator), id: value.slice(separator + 1) };
}

/**
 * Da chiamare su `pointerdown` di un elemento trascinabile. Non fa nulla
 * finché il puntatore non si sposta oltre la soglia: un clic normale resta
 * un clic.
 */
export function startNoteDrag(
  event: { button: number; clientX: number; clientY: number; pointerType?: string },
  options: DragOptions,
) {
  if (event.button !== 0 || event.pointerType === "touch") return;
  const startX = event.clientX;
  const startY = event.clientY;
  let ghost: HTMLElement | null = null;
  let active: HTMLElement | null = null;
  let scroller: HTMLElement | null = null;
  let scrollTimer = 0;
  let scrollStep = 0;

  const setActive = (element: HTMLElement | null) => {
    if (element === active) return;
    active?.removeAttribute("data-drop-active");
    active = element;
    active?.setAttribute("data-drop-active", "");
  };

  const autoScroll = (x: number, y: number) => {
    const sidebar = document.querySelector<HTMLElement>(".sidebar");
    if (!sidebar) return;
    const box = sidebar.getBoundingClientRect();
    const inside = x >= box.left && x <= box.right;
    scrollStep = !inside ? 0 : y < box.top + EDGE ? -12 : y > box.bottom - EDGE ? 12 : 0;
    scroller = sidebar;
    if (scrollStep !== 0 && scrollTimer === 0) {
      scrollTimer = window.setInterval(() => {
        if (scroller && scrollStep !== 0) scroller.scrollTop += scrollStep;
      }, 16);
    } else if (scrollStep === 0 && scrollTimer !== 0) {
      window.clearInterval(scrollTimer);
      scrollTimer = 0;
    }
  };

  const move = (pointer: PointerEvent) => {
    if (!ghost) {
      if (Math.hypot(pointer.clientX - startX, pointer.clientY - startY) < THRESHOLD) return;
      ghost = document.createElement("div");
      ghost.className = "note-drag-ghost";
      ghost.setAttribute("aria-hidden", "true");
      ghost.textContent = options.label;
      document.body.append(ghost);
      document.documentElement.dataset.dragging = "note";
    }
    ghost.style.transform = `translate(${pointer.clientX + 14}px, ${pointer.clientY + 10}px)`;
    const under = document.elementFromPoint(pointer.clientX, pointer.clientY);
    setActive(under?.closest<HTMLElement>("[data-drop]") ?? null);
    autoScroll(pointer.clientX, pointer.clientY);
  };

  const finish = (drop: boolean) => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", cancel);
    window.removeEventListener("keydown", key, true);
    if (scrollTimer !== 0) window.clearInterval(scrollTimer);
    const target = drop ? parseTarget(active?.dataset.drop) : null;
    setActive(null);
    if (ghost) {
      ghost.remove();
      delete document.documentElement.dataset.dragging;
      // Il rilascio genera un clic sull'elemento sotto il puntatore: dopo un
      // trascinamento non deve aprire la vista o la nota.
      // Il clic arriva subito dopo `pointerup`, nello stesso giro di eventi:
      // la protezione si toglie al giro successivo, così un rilascio fuori
      // dalla finestra (senza clic) non si mangia il clic seguente.
      const guard = (click: MouseEvent) => {
        click.preventDefault();
        click.stopPropagation();
      };
      window.addEventListener("click", guard, { capture: true, once: true });
      window.setTimeout(() => window.removeEventListener("click", guard, { capture: true }), 0);
    }
    if (target) options.onDrop(target);
  };
  const up = () => finish(true);
  const cancel = () => finish(false);
  const key = (keyboard: KeyboardEvent) => {
    if (keyboard.key === "Escape" && ghost) {
      keyboard.preventDefault();
      keyboard.stopPropagation();
      finish(false);
    }
  };

  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", cancel);
  window.addEventListener("keydown", key, true);
}
