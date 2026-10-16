import { create } from "zustand";

export type ToastTone = "success" | "info" | "caution" | "danger";

export interface ToastItem {
  id: number;
  tone: ToastTone;
  title: string;
  description?: string;
  /** Optional action, named by what it does. */
  action?: { label: string; run: () => void };
}

interface ToastStore {
  items: ToastItem[];
  push: (t: Omit<ToastItem, "id">) => void;
  dismiss: (id: number) => void;
}

let nextId = 1;
export const useToasts = create<ToastStore>((set) => ({
  items: [],
  push: (t) => set((s) => ({ items: [...s.items.slice(-2), { ...t, id: nextId++ }] })),
  dismiss: (id) => set((s) => ({ items: s.items.filter((x) => x.id !== id) })),
}));

export const toast = (t: Omit<ToastItem, "id">) => useToasts.getState().push(t);
