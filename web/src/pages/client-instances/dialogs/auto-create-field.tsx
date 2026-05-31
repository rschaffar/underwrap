import { FormControl, FormDescription, FormItem } from "@/components/ui/form";

/**
 * The "auto-create wrapper assets" checkbox shared by the create and
 * edit client-instance dialogs. Render it inside a `FormField` so the
 * surrounding field context is available to `FormControl`.
 */
export function AutoCreateField({
  checked,
  onChange,
  testId,
  extended = false,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  testId: string;
  /** Include the "when off …" sentence shown on the create dialog. */
  extended?: boolean;
}) {
  return (
    <FormItem>
      <label className="flex items-start gap-2 text-xs text-(--color-muted)">
        <FormControl>
          <input
            type="checkbox"
            data-testid={testId}
            checked={checked}
            onChange={(e) => onChange(e.target.checked)}
            className="mt-0.5"
          />
        </FormControl>
        <span>
          <strong className="text-(--color-fg)">
            Auto-create wrapper assets on register
          </strong>
          <br />
          <FormDescription>
            When on, this instance's wrappers may add new resources and
            wrapper/resource pairs on startup.
            {extended
              ? " When off, the backend refuses to create them and logs which entries it expected."
              : ""}
          </FormDescription>
        </span>
      </label>
    </FormItem>
  );
}
