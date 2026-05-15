import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

/**
 * Merge Tailwind class names: conditionally compose via `clsx`, then
 * collapse any later utilities that would override earlier ones via
 * `tailwind-merge`. The standard helper from the shadcn/ui template.
 */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
