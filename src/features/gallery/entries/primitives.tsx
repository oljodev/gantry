import { CaretDownIcon, DotsThreeIcon, GearIcon, PlusIcon, TrashIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from '@/components/ui/command';
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/components/ui/context-menu';
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Input } from '@/components/ui/input';
import { Kbd, KbdGroup } from '@/components/ui/kbd';
import {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverTitle,
  PopoverTrigger,
} from '@/components/ui/popover';
import { RadioGroup, RadioGroupItem, Segmented } from '@/components/ui/radio-group';
import { ScrollArea } from '@/components/ui/scroll-area';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Separator } from '@/components/ui/separator';
import { Skeleton } from '@/components/ui/skeleton';
import { Switch } from '@/components/ui/switch';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Textarea } from '@/components/ui/textarea';
import { toast } from '@/components/ui/toast';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { State, type GalleryEntry } from '@/features/gallery/types';

function Buttons() {
  return (
    <>
      <State label="Variants">
        <Button variant="primary">Send</Button>
        <Button variant="secondary">Cancel run</Button>
        <Button variant="ghost">Later</Button>
        <Button variant="danger">Delete chat</Button>
      </State>
      <State label="Sizes">
        <Button size="sm">Small</Button>
        <Button size="md">Medium</Button>
        <Button size="lg">Large</Button>
        <Button size="icon-sm" aria-label="More">
          <DotsThreeIcon />
        </Button>
        <Button size="icon-md" aria-label="Settings">
          <GearIcon />
        </Button>
        <Button size="icon-lg" variant="primary" aria-label="New">
          <PlusIcon />
        </Button>
      </State>
      <State label="With icon · disabled">
        <Button variant="secondary">
          <PlusIcon />
          Add key
        </Button>
        <Button variant="primary" disabled>
          Send
        </Button>
        <Button variant="secondary" disabled>
          Disabled
        </Button>
      </State>
    </>
  );
}

function Inputs() {
  return (
    <>
      <State label="Input">
        <Input placeholder="Search chats…" className="w-56" />
        <Input defaultValue="sk-ant-••••••••abcd" className="w-56" readOnly />
        <Input placeholder="Disabled" className="w-40" disabled />
        <Input defaultValue="not-a-url" aria-invalid className="w-40" />
      </State>
      <State label="Large">
        <Input size="lg" placeholder="Base URL" className="w-72" />
      </State>
      <State label="Textarea">
        <Textarea placeholder="Custom instructions…" className="w-96" />
      </State>
    </>
  );
}

function Choices() {
  const [seg, setSeg] = useState<'a' | 'b' | 'c'>('a');
  return (
    <>
      <State label="Checkbox">
        <label className="flex items-center gap-2 text-ui">
          <Checkbox defaultChecked /> Attached
        </label>
        <label className="flex items-center gap-2 text-ui">
          <Checkbox /> Off
        </label>
        <label className="flex items-center gap-2 text-ui">
          <Checkbox indeterminate /> Some
        </label>
        <label className="flex items-center gap-2 text-ui text-fg-disabled">
          <Checkbox disabled defaultChecked /> Disabled
        </label>
      </State>
      <State label="Switch">
        <Switch defaultChecked aria-label="On" />
        <Switch aria-label="Off" />
        <Switch disabled aria-label="Disabled" />
      </State>
      <State label="Radio">
        <RadioGroup defaultValue="judge" className="w-auto gap-3">
          <label className="flex items-center gap-2 text-ui">
            <RadioGroupItem value="judge" /> Judge on
          </label>
          <label className="flex items-center gap-2 text-ui">
            <RadioGroupItem value="off" /> Off
          </label>
        </RadioGroup>
      </State>
      <State label="Segmented">
        <Segmented
          aria-label="Example"
          value={seg}
          onValueChange={setSeg}
          options={[
            ['a', 'Unified'],
            ['b', 'Side by side'],
            ['c', 'Raw'],
          ]}
        />
      </State>
    </>
  );
}

function Selects() {
  return (
    <>
      <State label="Select">
        <Select defaultValue="claude-opus-5">
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="claude-opus-5">Claude Opus 5</SelectItem>
            <SelectItem value="claude-sonnet-5">Claude Sonnet 5</SelectItem>
            <SelectItem value="claude-haiku-4-5">Claude Haiku 4.5</SelectItem>
          </SelectContent>
        </Select>
        <Select>
          <SelectTrigger size="sm">
            <SelectValue placeholder="Pick a model" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="a">Option A</SelectItem>
          </SelectContent>
        </Select>
        <Select disabled defaultValue="x">
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="x">Disabled</SelectItem>
          </SelectContent>
        </Select>
      </State>
    </>
  );
}

function TabsEntry() {
  return (
    <State label="Line tabs">
      <Tabs defaultValue="artifact" className="w-96">
        <TabsList>
          <TabsTrigger value="artifact">Dashboard.tsx</TabsTrigger>
          <TabsTrigger value="diff">auth.rs · diff</TabsTrigger>
          <TabsTrigger value="cmd" disabled>
            cargo test
          </TabsTrigger>
        </TabsList>
        <TabsContent value="artifact" className="p-3 text-body text-fg-2">
          Artifact content
        </TabsContent>
        <TabsContent value="diff" className="p-3 text-body text-fg-2">
          Diff content
        </TabsContent>
      </Tabs>
    </State>
  );
}

function Floating() {
  return (
    <>
      <State label="Dialog">
        <Dialog>
          <DialogTrigger render={<Button variant="danger" />}>Delete chat</DialogTrigger>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Delete “Fix the failing auth tests”?</DialogTitle>
              <DialogDescription>
                The chat, its activity and its artifacts are removed. Files on disk are not touched.
              </DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <DialogClose render={<Button variant="secondary" />}>Keep</DialogClose>
              <Button variant="danger">Delete</Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </State>
      <State label="Popover · tooltip">
        <Popover>
          <PopoverTrigger render={<Button variant="secondary" />}>Turn details</PopoverTrigger>
          <PopoverContent>
            <PopoverTitle>Claude Opus 5</PopoverTitle>
            <PopoverDescription>14.2 s · 8,412 tokens in · 1,203 out</PopoverDescription>
          </PopoverContent>
        </Popover>
        <Tooltip>
          <TooltipTrigger render={<Button variant="ghost" size="icon-md" aria-label="Settings" />}>
            <GearIcon />
          </TooltipTrigger>
          <TooltipContent>
            Settings <Kbd>⌘,</Kbd>
          </TooltipContent>
        </Tooltip>
      </State>
      <State label="Dropdown · context menu">
        <DropdownMenu>
          <DropdownMenuTrigger render={<Button variant="secondary" />}>
            Auto-edit
            <CaretDownIcon />
          </DropdownMenuTrigger>
          <DropdownMenuContent>
            <DropdownMenuGroup>
              <DropdownMenuLabel>Permission mode</DropdownMenuLabel>
              <DropdownMenuItem>
                Manual<DropdownMenuShortcut>⇧⇥</DropdownMenuShortcut>
              </DropdownMenuItem>
              <DropdownMenuItem>Auto-edit</DropdownMenuItem>
              <DropdownMenuItem>Plan</DropdownMenuItem>
              <DropdownMenuItem>Auto</DropdownMenuItem>
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuCheckboxItem checked>Guard with judge</DropdownMenuCheckboxItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem variant="danger">
              <TrashIcon />
              Delete chat
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <ContextMenu>
          <ContextMenuTrigger className="flex h-(--control-lg) items-center rounded-2 border border-dashed border-line px-3 text-ui text-fg-2">
            Right-click me
          </ContextMenuTrigger>
          <ContextMenuContent>
            <ContextMenuItem>Pin</ContextMenuItem>
            <ContextMenuItem>Rename</ContextMenuItem>
            <ContextMenuItem>Move to project</ContextMenuItem>
            <ContextMenuSeparator />
            <ContextMenuItem variant="danger">Delete</ContextMenuItem>
          </ContextMenuContent>
        </ContextMenu>
      </State>
      <State label="Toast">
        <Button
          variant="secondary"
          onClick={() =>
            toast.add({
              title: 'Key saved',
              description: 'Anthropic · sk-ant-…abcd',
              type: 'success',
            })
          }
        >
          Success
        </Button>
        <Button
          variant="secondary"
          onClick={() => toast.add({ title: 'Could not reach api.openai.com', type: 'error' })}
        >
          Error
        </Button>
        <Button
          variant="secondary"
          onClick={() => toast.add({ title: 'Chat archived', actionProps: { children: 'Undo' } })}
        >
          With action
        </Button>
      </State>
    </>
  );
}

function Labels() {
  return (
    <>
      <State label="Badge">
        <Badge>MCP</Badge>
        <Badge variant="outline">Built in</Badge>
        <Badge variant="accent">2 pending</Badge>
        <Badge variant="good">Connected</Badge>
        <Badge variant="warn">Reconnect</Badge>
        <Badge variant="bad">Invalid key</Badge>
        <Badge variant="info">Summarized</Badge>
      </State>
      <State label="Kbd">
        <Kbd>⌘</Kbd>
        <KbdGroup>
          <Kbd>⌘</Kbd>
          <Kbd>K</Kbd>
        </KbdGroup>
        <KbdGroup>
          <Kbd>⇧</Kbd>
          <Kbd>⇥</Kbd>
        </KbdGroup>
      </State>
      <State label="Separator">
        <div className="flex w-64 flex-col gap-2">
          <span className="text-ui">Above</span>
          <Separator />
          <span className="text-ui">Below</span>
        </div>
      </State>
    </>
  );
}

function Loading() {
  return (
    <>
      <State label="Skeleton · a chat row and a message">
        <div className="flex w-72 flex-col gap-2">
          <Skeleton className="h-(--row-sidebar) w-full" />
          <Skeleton className="h-(--row-sidebar) w-4/5" />
          <Skeleton className="mt-2 h-4 w-full" />
          <Skeleton className="h-4 w-11/12" />
          <Skeleton className="h-4 w-2/3" />
        </div>
      </State>
      <State label="Scroll area">
        <ScrollArea className="h-32 w-64 rounded-3 border border-line">
          <div className="flex flex-col p-2">
            {Array.from({ length: 20 }, (_, i) => (
              <div key={i} className="flex h-(--row-sidebar) items-center px-2 text-ui">
                Row {i + 1}
              </div>
            ))}
          </div>
        </ScrollArea>
      </State>
    </>
  );
}

function CommandEntry() {
  return (
    <State label="Command list (inline)">
      <div className="float w-96">
        <Command>
          <CommandInput placeholder="Search chats, settings, actions…" autoFocus={false} />
          <CommandList>
            <CommandEmpty>Nothing matches.</CommandEmpty>
            <CommandGroup heading="Actions">
              <CommandItem>
                <PlusIcon />
                New chat<CommandShortcut>⌘N</CommandShortcut>
              </CommandItem>
              <CommandItem>
                <GearIcon />
                Open settings<CommandShortcut>⌘,</CommandShortcut>
              </CommandItem>
            </CommandGroup>
            <CommandGroup heading="Chats">
              <CommandItem>Fix the failing auth tests</CommandItem>
              <CommandItem>Draft the release notes</CommandItem>
            </CommandGroup>
          </CommandList>
        </Command>
      </div>
    </State>
  );
}

export const primitiveEntries: GalleryEntry[] = [
  { id: 'button', title: 'Button', group: 'Primitives', render: () => <Buttons /> },
  { id: 'input', title: 'Input · Textarea', group: 'Primitives', render: () => <Inputs /> },
  {
    id: 'choices',
    title: 'Checkbox · Switch · Radio · Segmented',
    group: 'Primitives',
    render: () => <Choices />,
  },
  { id: 'select', title: 'Select', group: 'Primitives', render: () => <Selects /> },
  { id: 'tabs', title: 'Tabs', group: 'Primitives', render: () => <TabsEntry /> },
  {
    id: 'floating',
    title: 'Dialog · Popover · Menus · Toast',
    group: 'Primitives',
    render: () => <Floating />,
  },
  { id: 'labels', title: 'Badge · Kbd · Separator', group: 'Primitives', render: () => <Labels /> },
  {
    id: 'loading',
    title: 'Skeleton · Scroll area',
    group: 'Primitives',
    render: () => <Loading />,
  },
  { id: 'command', title: 'Command', group: 'Primitives', render: () => <CommandEntry /> },
];
