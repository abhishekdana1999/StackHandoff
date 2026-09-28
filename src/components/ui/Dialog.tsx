import { ReactNode, forwardRef, HTMLAttributes, useCallback, useEffect } from 'react';
import { clsx } from 'clsx';
import { X } from 'lucide-react';
import { createPortal } from 'react-dom';

interface DialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  children: ReactNode;
  /** Applied to the dialog panel. It used to be applied to the scrim as well. */
  className?: string;
  title?: string;
  description?: string;
}

export function Dialog({ open, onOpenChange, children, className, title, description }: DialogProps) {
  /**
   * Escape closes the dialog.
   *
   * This was written as a `handleKeyDown` function that the effect then failed to
   * register — the listener was an inline arrow that did nothing with the key.
   * So the close button and the scrim worked and the keyboard did not, which is
   * the one dismissal path a desktop user expects to be guaranteed.
   */
  useEffect(() => {
    if (!open) return;

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        onOpenChange(false);
      }
    };

    document.addEventListener('keydown', onKeyDown);
    // The window behind must not scroll while a modal is up.
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';

    return () => {
      document.removeEventListener('keydown', onKeyDown);
      document.body.style.overflow = previousOverflow;
    };
  }, [open, onOpenChange]);

  const close = useCallback(() => onOpenChange(false), [onOpenChange]);

  // The early return has to come *after* every hook. Returning null above the
  // effect meant the hook count changed between the closed and open renders of
  // the same component instance, which React does not allow.
  if (!open) return null;

  return createPortal(
    <>
      <div
        className="fixed inset-0 z-50 bg-black/40 dark:bg-black/60"
        onClick={close}
        aria-hidden="true"
      />
      <div
        className={clsx(
          // A scrim, plus a hairline. `shadow-lg` was the only shadow in the
          // app and it belonged here, but it is dialled back and tinted to
          // match a near-black canvas rather than black.
          'fixed left-1/2 top-1/2 z-50 grid w-[calc(100vw-2rem)] max-w-lg -translate-x-1/2 -translate-y-1/2 gap-3',
          'rounded-lg border border-border bg-surface-overlay p-4',
          'shadow-[0_16px_48px_-12px_hsl(0_0%_0%/0.55)]',
          className
        )}
        role="dialog"
        aria-modal="true"
        aria-labelledby={title ? 'dialog-title' : undefined}
        aria-describedby={description ? 'dialog-description' : undefined}
      >
        <button
          type="button"
          className="absolute right-3 top-3 flex h-6 w-6 items-center justify-center rounded text-fg-subtle transition-colors hover:bg-surface-hover hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none"
          onClick={close}
          aria-label="Close"
        >
          <X className="h-3.5 w-3.5" aria-hidden="true" />
        </button>
        {(title || description) && (
          <div className="grid gap-1 pr-6">
            {title && (
              <h2 id="dialog-title" className="text-[15px] font-semibold leading-tight">
                {title}
              </h2>
            )}
            {description && (
              <p id="dialog-description" className="text-[13px] text-muted-foreground">
                {description}
              </p>
            )}
          </div>
        )}
        {children}
      </div>
    </>,
    document.body
  );
}

interface DialogContentProps extends HTMLAttributes<HTMLDivElement> {
  children: ReactNode;
}

export const DialogContent = forwardRef<HTMLDivElement, DialogContentProps>(
  ({ className, children, ...props }, ref) => (
    <div ref={ref} className={clsx('grid gap-2', className)} {...props}>
      {children}
    </div>
  )
);
DialogContent.displayName = 'DialogContent';
