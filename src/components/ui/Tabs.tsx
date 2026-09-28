import { createContext, useContext, useState, ReactNode, HTMLAttributes } from 'react';
import { clsx } from 'clsx';

interface TabsContextValue {
  value: string;
  onValueChange: (value: string) => void;
}

const TabsContext = createContext<TabsContextValue | null>(null);

function useTabsContext() {
  const context = useContext(TabsContext);
  if (!context) {
    throw new Error('Tabs components must be used within Tabs');
  }
  return context;
}

interface TabsCommonProps {
  children: ReactNode;
  onValueChange?: (value: string) => void;
  className?: string;
}

/**
 * Exactly one of the two must be given, and the type says so.
 *
 * A union rather than two optional props, because "neither" is a real mistake:
 * an uncontrolled Tabs with no initial value has no panel showing and no error,
 * just an empty frame the user cannot get out of. "Both" is the other mistake,
 * and it is worse -- the controlled value wins and `defaultValue` silently rots.
 * Deriving the first trigger's value was the other option, and it cannot work:
 * the triggers are nested inside `TabsList`, so they are not visible from here.
 */
type TabsProps =
  | (TabsCommonProps & { value: string; defaultValue?: never })
  | (TabsCommonProps & { defaultValue: string; value?: never });

export function Tabs({ children, defaultValue, value: controlledValue, onValueChange, className }: TabsProps) {
  const isControlled = controlledValue !== undefined;
  // `''` is unreachable for an uncontrolled Tabs, whose type demands a
  // `defaultValue`; it only types the state for the controlled case, where
  // `uncontrolledValue` is never read.
  const [uncontrolledValue, setUncontrolledValue] = useState<string>(defaultValue ?? '');
  const value = isControlled ? (controlledValue as string) : uncontrolledValue;

  const handleValueChange = (newValue: string) => {
    if (!isControlled) {
      setUncontrolledValue(newValue);
    }
    onValueChange?.(newValue);
  };

  return (
    <TabsContext.Provider value={{ value, onValueChange: handleValueChange }}>
      <div className={clsx('space-y-4', className)} data-slot="tabs">{children}</div>
    </TabsContext.Provider>
  );
}

interface TabsListProps extends HTMLAttributes<HTMLDivElement> {}

export function TabsList({ className, children, ...props }: TabsListProps) {
  return (
    <div
      role="tablist"
      aria-orientation="horizontal"
      className={clsx('inline-flex h-10 items-center justify-center rounded-md bg-muted p-1 text-muted-foreground', className)}
      {...props}
    >
      {children}
    </div>
  );
}

interface TabsTriggerProps extends HTMLAttributes<HTMLButtonElement> {
  value: string;
  disabled?: boolean;
}

export function TabsTrigger({ className, value, disabled, children, ...props }: TabsTriggerProps) {
  const { value: contextValue, onValueChange } = useTabsContext();
  const isActive = contextValue === value;

  return (
    <button
      role="tab"
      aria-selected={isActive}
      aria-controls={`tabs-${value}-panel`}
      id={`tabs-${value}-trigger`}
      data-state={isActive ? 'active' : 'inactive'}
      data-disabled={disabled ? '' : undefined}
      className={clsx(
        // A segmented control, macOS style: 26px, no shadow on the active
        // segment. The `shadow-sm` here was the shadcn lift; on a 26px row it
        // read as a border being drawn twice.
        'inline-flex h-[26px] items-center justify-center whitespace-nowrap rounded px-2.5 text-[13px] font-medium transition-colors',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
        'disabled:pointer-events-none disabled:opacity-50',
        'data-[state=active]:bg-surface data-[state=active]:text-foreground data-[state=active]:font-semibold',
        'data-[state=inactive]:text-fg-subtle data-[state=inactive]:hover:text-foreground',
        className
      )}
      onClick={() => !disabled && onValueChange(value)}
      disabled={disabled}
      {...props}
    >
      {children}
    </button>
  );
}

interface TabsContentProps extends HTMLAttributes<HTMLDivElement> {
  value: string;
}

export function TabsContent({ className, value, children, ...props }: TabsContentProps) {
  const { value: contextValue } = useTabsContext();
  const isActive = contextValue === value;

  if (!isActive) return null;

  return (
    <div
      role="tabpanel"
      id={`tabs-${value}-panel`}
      aria-labelledby={`tabs-${value}-trigger`}
      className={clsx('mt-2 ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2', className)}
      {...props}
    >
      {children}
    </div>
  );
}