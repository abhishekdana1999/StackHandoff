import { ReactNode } from 'react';
import { clsx } from 'clsx';

interface EmptyStateProps {
  /** Small muted glyph. Deliberately 20px, not 48px. */
  icon?: ReactNode;
  title: string;
  children: ReactNode;
  action?: ReactNode;
}

/**
 * What a list shows when it has nothing in it.
 *
 * A first-run user has zero workspaces, zero paired devices and zero transfers
 * at the same time, so these are not edge cases — they are the first thing the
 * app shows three times over. Each one says what the list is *for* and offers
 * the single action that fills it.
 *
 * The 64px circle with a 32px glyph that this replaces was a web empty-state
 * illustration: it pushed the actual explanation below the fold in a 780px
 * window and gave a list that is simply empty the visual weight of a headline.
 * A 20px muted icon beside the sentence says the same thing without pretending
 * the absence is content.
 */
export function EmptyState({ icon, title, children, action }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center gap-2 px-6 py-10 text-center">
      {icon && (
        <div className="mb-1 text-fg-faint" aria-hidden="true">
          {icon}
        </div>
      )}
      <h3 className="text-[15px] font-semibold leading-tight">{title}</h3>
      <div className="max-w-md text-[13px] text-muted-foreground">{children}</div>
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

/**
 * A card that has nothing to show.
 *
 * The card wrapper is separate because a list's empty state should sit in the
 * same container its rows would have sat in — otherwise the layout jumps the
 * moment the first row arrives.
 */
export function EmptyStateCard(props: EmptyStateProps & { className?: string }) {
  const { className, ...rest } = props;
  return (
    <div
      className={clsx(
        'rounded-md border border-dashed border-border bg-surface',
        className
      )}
    >
      <EmptyState {...rest} />
    </div>
  );
}
