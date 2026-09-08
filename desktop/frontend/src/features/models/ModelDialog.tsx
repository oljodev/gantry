import { Dialog as DialogPrimitive } from '@base-ui/react/dialog';
import {
  BrainIcon,
  CheckIcon,
  EyeIcon,
  FilePdfIcon,
  MagnifyingGlassIcon,
  StarIcon,
  WrenchIcon,
  XIcon,
} from '@phosphor-icons/react';
import { useMemo, useState } from 'react';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import type { ModelRef } from '@/bindings';
import {
  contextLabel,
  creatorsOf,
  isFiltered,
  KIND_LABEL,
  matches,
  meets,
  modelKey,
  NEED_LABEL,
  NO_FILTERS,
  priceLabel,
  SORT_LABEL,
  sortModels,
  toCatalog,
  type CatalogModel,
  type Filters,
  type ModelKind,
  type Need,
  type Sort,
} from '@/features/models/catalog';
import { useModelCatalog } from '@/lib/ipc/hooks/providers';
import { useSettings, useUpdateSettings } from '@/lib/ipc/hooks/settings';
import { chatDefaults } from '@/lib/settingsDefaults';
import { useUiStore } from '@/lib/stores/uiStore';
import { cn } from '@/lib/utils';

const KINDS: ModelKind[] = ['text', 'image', 'audio', 'video'];
const NEEDS: Need[] = ['vision', 'files', 'tools', 'reasoning', 'caching'];
const SORTS: Sort[] = ['name', 'price', 'context'];
/** Creators past this are behind "Show all": the tail is a long list of one-model names. */
const CREATORS_SHOWN = 8;

/**
 * Choosing a model (docs/plan/15 §7, added 2026-09-08): a dialog on the settings frame rather
 * than a drop-up on the composer.
 *
 * A list of four hundred models is not a menu, it is a catalog, and the questions people ask of
 * it — who made this, what does it produce, can it see a picture, what does it cost, how much
 * room does it have — are questions a popover cannot answer while staying a popover. So the
 * facets live down the left, the answers live in the row, and the models the user keeps coming
 * back to sit at the top so the common case is still two clicks.
 */
export function ModelDialog({
  open,
  onClose,
  value,
  onChange,
}: {
  open: boolean;
  onClose: () => void;
  value: ModelRef;
  onChange: (model: ModelRef) => void;
}) {
  const { providers, isPending } = useModelCatalog();
  const settings = useSettings();
  const update = useUpdateSettings();
  const recent = useUiStore((s) => s.recentModels);
  const remember = useUiStore((s) => s.rememberModel);

  const [filters, setFilters] = useState<Filters>(NO_FILTERS);
  const [sort, setSort] = useState<Sort>('name');
  const [allCreators, setAllCreators] = useState(false);

  const chat = chatDefaults(settings.data);
  const favourites = chat.favourite_models;
  const models = useMemo(() => toCatalog(providers), [providers]);
  const shown = useMemo(
    () =>
      sortModels(
        models.filter((m) => matches(m, filters)),
        sort,
      ),
    [models, filters, sort],
  );
  const creators = useMemo(() => creatorsOf(models), [models]);

  const set = (patch: Partial<Filters>) => setFilters((f) => ({ ...f, ...patch }));
  const toggle = <T,>(list: T[], item: T): T[] =>
    list.includes(item) ? list.filter((x) => x !== item) : [...list, item];

  const choose = (m: CatalogModel) => {
    onChange(m.ref);
    remember(m.key);
    onClose();
  };

  const star = (m: CatalogModel) => {
    const has = favourites.some((f) => modelKey(f) === m.key);
    update.mutate({
      chat: {
        ...chat,
        favourite_models: has
          ? favourites.filter((f) => modelKey(f) !== m.key)
          : [m.ref, ...favourites],
      },
    });
  };

  const byKey = new Map(models.map((m) => [m.key, m]));
  const pinned = isFiltered(filters)
    ? []
    : favourites
        .map((f) => byKey.get(modelKey(f)))
        .filter((m): m is CatalogModel => m !== undefined);
  const recents = isFiltered(filters)
    ? []
    : recent
        .filter((k) => !favourites.some((f) => modelKey(f) === k))
        .map((k) => byKey.get(k))
        .filter((m): m is CatalogModel => m !== undefined)
        .slice(0, 5);

  const row = (m: CatalogModel) => (
    <ModelRow
      key={m.key}
      model={m}
      selected={m.ref.provider === value.provider && m.ref.model === value.model}
      favourite={favourites.some((f) => modelKey(f) === m.key)}
      showProvider={providers.filter((p) => p.hasKey).length > 1}
      onChoose={() => choose(m)}
      onStar={() => star(m)}
    />
  );

  return (
    <DialogPrimitive.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Backdrop className="backdrop-anim fixed inset-0 z-50 bg-backdrop" />
        <DialogPrimitive.Popup
          className="float dialog-anim fixed top-1/2 left-1/2 z-50 flex h-(--prefs-height) w-(--prefs-width) -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden bg-raised p-0 text-ui text-fg outline-none"
          aria-label="Choose a model"
        >
          <DialogPrimitive.Title className="sr-only">Choose a model</DialogPrimitive.Title>

          <header className="flex shrink-0 items-center gap-2 border-b border-line-subtle px-4 py-3">
            <label className="relative min-w-0 flex-1">
              <MagnifyingGlassIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-fg-3" />
              <input
                type="search"
                autoFocus
                value={filters.query}
                onChange={(e) => set({ query: e.target.value })}
                onKeyDown={(e) => {
                  // Type three letters, press Enter: the top match is almost always the one.
                  if (e.key === 'Enter' && shown[0]) choose(shown[0]);
                }}
                placeholder="Search models, creators or ids"
                className="h-(--control-md) w-full rounded-2 border border-line bg-surface pr-2 pl-8 text-ui text-fg placeholder:text-fg-3 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-focus"
              />
            </label>
            <Select value={sort} onValueChange={(v) => v && setSort(v as Sort)}>
              <SelectTrigger aria-label="Sort models" className="shrink-0">
                <SelectValue>{(v: Sort) => SORT_LABEL[v]}</SelectValue>
              </SelectTrigger>
              <SelectContent>
                {SORTS.map((s) => (
                  <SelectItem key={s} value={s}>
                    {SORT_LABEL[s]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <DialogPrimitive.Close
              aria-label="Close"
              render={<Button variant="ghost" size="icon-sm" />}
            >
              <XIcon />
            </DialogPrimitive.Close>
          </header>

          <div className="flex min-h-0 flex-1">
            <nav
              aria-label="Filters"
              className="flex w-(--settings-list) shrink-0 flex-col gap-5 overflow-y-auto border-r border-line-subtle bg-base p-3"
            >
              <Group title="Makes">
                {KINDS.map((k) => {
                  const count = models.filter((m) => m.kind === k).length;
                  if (count === 0) return null;
                  return (
                    <FilterRow
                      key={k}
                      label={KIND_LABEL[k]}
                      count={count}
                      checked={filters.kinds.includes(k)}
                      onChange={() => set({ kinds: toggle(filters.kinds, k) })}
                    />
                  );
                })}
              </Group>

              <Group title="Can">
                {NEEDS.map((need) => (
                  <FilterRow
                    key={need}
                    label={NEED_LABEL[need]}
                    count={models.filter((m) => meets(m.info.capabilities, need)).length}
                    checked={filters.needs.includes(need)}
                    onChange={() => set({ needs: toggle(filters.needs, need) })}
                  />
                ))}
              </Group>

              <Group title="Price">
                <label className="flex h-(--row) items-center justify-between gap-2 px-1 text-ui text-fg">
                  Free only
                  <Switch
                    checked={filters.freeOnly}
                    onCheckedChange={(on) => set({ freeOnly: on })}
                    aria-label="Free models only"
                  />
                </label>
              </Group>

              <Group title="Creator">
                {(allCreators ? creators : creators.slice(0, CREATORS_SHOWN)).map((c) => (
                  <FilterRow
                    key={c.name}
                    label={c.name}
                    count={c.count}
                    checked={filters.creators.includes(c.name)}
                    onChange={() => set({ creators: toggle(filters.creators, c.name) })}
                  />
                ))}
                {creators.length > CREATORS_SHOWN && (
                  <button
                    type="button"
                    onClick={() => setAllCreators((v) => !v)}
                    className="px-1 py-1 text-left text-meta text-fg-3 transition-colors duration-(--dur-1) hover:text-fg"
                  >
                    {allCreators ? 'Show fewer' : `Show all ${creators.length}`}
                  </button>
                )}
              </Group>

              {isFiltered(filters) && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="mt-auto"
                  onClick={() => setFilters(NO_FILTERS)}
                >
                  Clear filters
                </Button>
              )}
            </nav>

            <div className="min-w-0 flex-1 overflow-y-auto">
              {isPending && <Empty>Loading the model list…</Empty>}
              {!isPending && models.length === 0 && (
                <Empty>
                  No models yet. Add a provider key in Settings → Providers, then refresh its list.
                </Empty>
              )}
              {!isPending && models.length > 0 && shown.length === 0 && (
                <Empty>Nothing matches those filters.</Empty>
              )}
              {pinned.length > 0 && (
                <Section title="Favourites">{pinned.map((m) => row(m))}</Section>
              )}
              {recents.length > 0 && <Section title="Recent">{recents.map((m) => row(m))}</Section>}
              {shown.length > 0 && (
                <Section title={isFiltered(filters) ? `${shown.length} models` : 'All models'}>
                  {shown.map((m) => row(m))}
                </Section>
              )}
            </div>
          </div>

          {/* Which upstream serves the model is OpenRouter's business, and saying so once is all
              the app has to say about it. */}
          <footer className="shrink-0 border-t border-line-subtle px-4 py-2 text-meta text-fg-3">
            {providers
              .filter((p) => p.hasKey)
              .map((p) => p.label)
              .join(', ') || 'No provider'}{' '}
            · routing chosen automatically
          </footer>
        </DialogPrimitive.Popup>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col">
      <div className="px-1 pb-1 text-micro font-medium uppercase tracking-[0.04em] text-fg-3">
        {title}
      </div>
      {children}
    </div>
  );
}

function FilterRow({
  label,
  count,
  checked,
  onChange,
}: {
  label: string;
  count: number;
  checked: boolean;
  onChange: () => void;
}) {
  return (
    <label className="flex h-(--row) items-center gap-2 rounded-2 px-1 text-ui text-fg transition-colors duration-(--dur-1) hover:bg-hover">
      <Checkbox checked={checked} onCheckedChange={onChange} />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      <span className="shrink-0 text-meta text-fg-3 tnum">{count}</span>
    </label>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section>
      <h2 className="sticky top-0 z-10 bg-raised px-4 py-1.5 text-micro font-medium uppercase tracking-[0.04em] text-fg-3">
        {title}
      </h2>
      {children}
    </section>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return <p className="px-4 py-6 text-body text-fg-2">{children}</p>;
}

function ModelRow({
  model,
  selected,
  favourite,
  showProvider,
  onChoose,
  onStar,
}: {
  model: CatalogModel;
  selected: boolean;
  favourite: boolean;
  showProvider: boolean;
  onChoose: () => void;
  onStar: () => void;
}) {
  const caps = model.info.capabilities;
  return (
    <div
      className={cn(
        'group flex items-center gap-2 border-b border-line-subtle/60 pr-3 pl-1',
        selected && 'bg-selected',
      )}
    >
      <button
        type="button"
        aria-label={favourite ? `Unstar ${model.name}` : `Star ${model.name}`}
        aria-pressed={favourite}
        onClick={onStar}
        className={cn(
          'flex size-7 shrink-0 items-center justify-center rounded-2 transition-colors duration-(--dur-1) hover:bg-hover',
          favourite ? 'text-warn' : 'text-fg-3 opacity-0 group-hover:opacity-100 focus:opacity-100',
        )}
      >
        <StarIcon size={14} weight={favourite ? 'fill' : 'regular'} />
      </button>
      <button
        type="button"
        onClick={onChoose}
        className="flex min-w-0 flex-1 items-center gap-3 py-2 text-left"
      >
        <span className="min-w-0 flex-1">
          <span className="flex items-baseline gap-2">
            <span className="truncate text-ui text-fg">{model.name}</span>
            <span className="shrink-0 text-meta text-fg-3">{model.creator}</span>
            {model.kind !== 'text' && (
              <Badge variant="accent" className="shrink-0">
                {KIND_LABEL[model.kind]}
              </Badge>
            )}
          </span>
          <span className="flex items-center gap-2">
            <span className="truncate font-mono text-micro text-fg-3">{model.id}</span>
            {showProvider && (
              <span className="shrink-0 text-micro text-fg-3">{model.providerLabel}</span>
            )}
          </span>
        </span>
        <span className="flex shrink-0 items-center gap-1 text-fg-3">
          {meets(caps, 'vision') && <EyeIcon size={13} aria-label="Reads images" />}
          {meets(caps, 'files') && <FilePdfIcon size={13} aria-label="Reads files" />}
          {meets(caps, 'tools') && <WrenchIcon size={13} aria-label="Calls tools" />}
          {meets(caps, 'reasoning') && <BrainIcon size={13} aria-label="Thinks first" />}
        </span>
        <span className="w-14 shrink-0 text-right text-meta text-fg-2 tnum">
          {contextLabel(model.info.context_window)}
        </span>
        <span className="w-32 shrink-0 text-right text-meta text-fg-2 tnum">
          {priceLabel(model)}
        </span>
        <span className="flex w-4 shrink-0 justify-center">
          {selected && <CheckIcon size={14} className="text-accent-text" />}
        </span>
      </button>
    </div>
  );
}
