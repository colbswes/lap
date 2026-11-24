// Finder-style click selection for the People/Pets sidebar list.
export interface PersonClickModifiers {
  shiftKey?: boolean;
  metaKey?: boolean;
  ctrlKey?: boolean;
}

export interface PersonSelectionState {
  /** Selected ids, in list order. */
  ids: number[];
  /** Fixed end of a Shift-click range. */
  anchorId: number | null;
}

export function nextPersonSelection(
  orderedIds: number[],
  current: PersonSelectionState,
  clickedId: number,
  modifiers: PersonClickModifiers = {},
): PersonSelectionState {
  const toggle = !!(modifiers.metaKey || modifiers.ctrlKey);
  const anchorIndex = current.anchorId === null ? -1 : orderedIds.indexOf(current.anchorId);
  const clickedIndex = orderedIds.indexOf(clickedId);
  const selected = new Set(current.ids);

  if (modifiers.shiftKey && anchorIndex >= 0 && clickedIndex >= 0) {
    const [start, end] = anchorIndex <= clickedIndex ? [anchorIndex, clickedIndex] : [clickedIndex, anchorIndex];
    const range = new Set(toggle ? selected : []);
    for (let i = start; i <= end; i++) range.add(orderedIds[i]);
    return { ids: orderedIds.filter(id => range.has(id)), anchorId: current.anchorId };
  }

  if (toggle) {
    if (selected.has(clickedId)) selected.delete(clickedId);
    else selected.add(clickedId);
    return { ids: orderedIds.filter(id => selected.has(id)), anchorId: clickedId };
  }

  return { ids: [clickedId], anchorId: clickedId };
}
