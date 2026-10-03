import { useCallback, useEffect, useState } from 'react';

import { Icon } from './Icon';
import type { IconName } from './Icon';

export interface ToastItem {
  id: number;
  kind: 'info' | 'success' | 'error';
  message: string;
}

let seq = 0;

export function useToasts() {
  const [toasts, setToasts] = useState<ToastItem[]>([]);

  const dismiss = useCallback((id: number) => {
    setToasts((list) => list.filter((t) => t.id !== id));
  }, []);

  const push = useCallback((kind: ToastItem['kind'], message: string) => {
    seq += 1;
    const id = seq;
    setToasts((list) => [...list, { id, kind, message }].slice(-4));
    return id;
  }, []);

  return {
    toasts,
    dismiss,
    info: useCallback((m: string) => push('info', m), [push]),
    success: useCallback((m: string) => push('success', m), [push]),
    error: useCallback((m: string) => push('error', m), [push]),
  };
}

function Toast({ item, onClose }: { item: ToastItem; onClose: () => void }) {
  useEffect(() => {
    // 错误信息停留更久，用户需要时间读完。
    const timer = setTimeout(onClose, item.kind === 'error' ? 9000 : 4200);
    return () => clearTimeout(timer);
  }, [item.kind, onClose]);

  const icon: IconName = item.kind === 'error' ? 'cross' : item.kind === 'success' ? 'check' : 'eye';

  return (
    <div className={`toast ${item.kind}`} role="status">
      <Icon name={icon} size={15} />
      <span>{item.message}</span>
      <button className="close" onClick={onClose} title="关闭">
        <Icon name="close" size={13} />
      </button>
    </div>
  );
}

export function Toasts({
  items,
  onDismiss,
}: {
  items: ToastItem[];
  onDismiss: (id: number) => void;
}) {
  return (
    <div className="toasts">
      {items.map((t) => (
        <Toast key={t.id} item={t} onClose={() => onDismiss(t.id)} />
      ))}
    </div>
  );
}
