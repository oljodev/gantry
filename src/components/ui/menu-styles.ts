/** Class recipes shared by dropdown, context and select menus (15 §6 level 2, §8). */
export const menuPopup =
  'float pop-anim z-50 max-h-(--available-height) min-w-40 overflow-x-hidden overflow-y-auto p-1 text-ui text-fg outline-none';

export const menuItem =
  'relative flex h-(--control-md) cursor-default select-none items-center gap-2 rounded-2 px-2 text-ui text-fg outline-none data-highlighted:bg-hover data-disabled:pointer-events-none data-disabled:text-fg-disabled [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-fg-2';

export const menuItemDanger = 'text-bad data-highlighted:bg-bad-subtle [&_svg]:text-bad';

export const menuLabel =
  'px-2 pt-2 pb-1 text-micro font-medium uppercase tracking-[0.04em] text-fg-3';

export const menuSeparator = '-mx-1 my-1 h-px bg-line-subtle';

export const menuShortcut = 'ml-auto pl-4 text-meta text-fg-3';
