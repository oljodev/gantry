import { Tabs as TabsPrimitive } from '@base-ui/react/tabs';

import { cn } from '@/lib/utils';

/** Line tabs: `ui` 500 labels, the active one carrying a 2 px underline in `fg` (15 §7 pane tabs). */
function Tabs({ className, ...props }: TabsPrimitive.Root.Props) {
  return (
    <TabsPrimitive.Root data-slot="tabs" className={cn('flex flex-col', className)} {...props} />
  );
}

function TabsList({ className, ...props }: TabsPrimitive.List.Props) {
  return (
    <TabsPrimitive.List
      data-slot="tabs-list"
      className={cn('relative flex h-(--row) items-end gap-1 border-b border-line px-1', className)}
      {...props}
    />
  );
}

function TabsTrigger({ className, ...props }: TabsPrimitive.Tab.Props) {
  return (
    <TabsPrimitive.Tab
      data-slot="tabs-trigger"
      className={cn(
        'relative -mb-px flex h-(--control-lg) items-center gap-1.5 whitespace-nowrap border-b-2 border-transparent px-2 text-ui font-medium text-fg-2 transition-colors duration-(--dur-1) hover:text-fg data-active:border-fg data-active:text-fg data-disabled:pointer-events-none data-disabled:text-fg-disabled [&_svg]:size-3.5 [&_svg]:shrink-0',
        className,
      )}
      {...props}
    />
  );
}

function TabsContent({ className, ...props }: TabsPrimitive.Panel.Props) {
  return (
    <TabsPrimitive.Panel
      data-slot="tabs-content"
      className={cn('flex-1 outline-none', className)}
      {...props}
    />
  );
}

export { Tabs, TabsContent, TabsList, TabsTrigger };
