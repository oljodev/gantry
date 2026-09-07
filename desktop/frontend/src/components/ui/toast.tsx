import { Toast as ToastPrimitive } from '@base-ui/react/toast';
import { CheckCircleIcon, InfoIcon, WarningIcon, XCircleIcon, XIcon } from '@phosphor-icons/react';
import type * as React from 'react';

import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

/**
 * Toasts: bottom right, level 2, `ui` size, auto-dismiss, one action at most (15 §8). Transient
 * confirmations and background failures only; errors that belong somewhere render inline.
 */
const toast = ToastPrimitive.createToastManager();

function ToastList() {
  const { toasts } = ToastPrimitive.useToastManager();
  return toasts.map((t) => (
    <ToastPrimitive.Root
      key={t.id}
      toast={t}
      data-slot="toast"
      className={cn(
        'float pointer-events-auto absolute right-0 bottom-0 z-50 w-full select-none rounded-3 p-3 text-ui text-fg',
        'translate-y-[calc(var(--toast-offset-y)*-1+var(--toast-index)*-8px)] transition-[transform,opacity] duration-(--dur-3) ease-out data-[ending-style]:opacity-0 data-[starting-style]:translate-y-2 data-[starting-style]:opacity-0',
      )}
    >
      <ToastPrimitive.Content className="flex items-start gap-2.5">
        <ToastIcon type={t.type} />
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <ToastPrimitive.Title className="text-ui font-medium" />
          <ToastPrimitive.Description className="text-meta text-fg-2" />
        </div>
        <ToastPrimitive.Action
          render={<Button variant="secondary" size="sm" />}
          className="shrink-0"
        />
        <ToastPrimitive.Close
          aria-label="Close"
          render={<Button variant="ghost" size="icon-sm" />}
          className="-mr-1 -mt-1 shrink-0"
        >
          <XIcon />
        </ToastPrimitive.Close>
      </ToastPrimitive.Content>
    </ToastPrimitive.Root>
  ));
}

function ToastIcon({ type }: { type: string | undefined }) {
  const icon: React.ReactNode =
    type === 'success' ? (
      <CheckCircleIcon className="text-good" weight="fill" />
    ) : type === 'info' ? (
      <InfoIcon className="text-info" weight="fill" />
    ) : type === 'warning' ? (
      <WarningIcon className="text-warn" weight="fill" />
    ) : type === 'error' ? (
      <XCircleIcon className="text-bad" weight="fill" />
    ) : null;
  if (!icon) return null;
  return <span className="mt-px shrink-0 [&_svg]:size-4">{icon}</span>;
}

/** Mount once near the root. */
function Toaster({ children, ...props }: Omit<ToastPrimitive.Provider.Props, 'toastManager'>) {
  return (
    <ToastPrimitive.Provider toastManager={toast} timeout={5000} {...props}>
      {children}
      <ToastPrimitive.Portal>
        <ToastPrimitive.Viewport className="pointer-events-none fixed right-4 bottom-4 z-50 w-80 outline-none">
          <ToastList />
        </ToastPrimitive.Viewport>
      </ToastPrimitive.Portal>
    </ToastPrimitive.Provider>
  );
}

const useToastManager = ToastPrimitive.useToastManager;

export { toast, Toaster, useToastManager };
