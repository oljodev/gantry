import type { ChatSummary } from '@/fixtures/types';

/** Today / Yesterday / Previous 7 days / Previous 30 days / Older (01 §5). */
export function groupByDay(
  chats: ChatSummary[],
  now = new Date(),
): { label: string; chats: ChatSummary[] }[] {
  const day = 86_400_000;
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const buckets: { label: string; from: number }[] = [
    { label: 'Today', from: startOfToday },
    { label: 'Yesterday', from: startOfToday - day },
    { label: 'Previous 7 days', from: startOfToday - 7 * day },
    { label: 'Previous 30 days', from: startOfToday - 30 * day },
    { label: 'Older', from: -Infinity },
  ];
  const sorted = [...chats].sort((a, b) => b.lastMessageAt.localeCompare(a.lastMessageAt));
  const groups = buckets.map((b) => ({ label: b.label, chats: [] as ChatSummary[] }));
  for (const chat of sorted) {
    const t = Date.parse(chat.lastMessageAt);
    const idx = buckets.findIndex((b) => t >= b.from);
    groups[idx === -1 ? groups.length - 1 : idx]!.chats.push(chat);
  }
  return groups.filter((g) => g.chats.length > 0);
}
