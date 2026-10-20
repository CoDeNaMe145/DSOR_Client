// CPAL owns its AudioContext internally. Remember contexts at construction so
// resume() can run directly inside a trusted browser gesture (also on Safari).
(() => {
  const NativeAudioContext = window.AudioContext;
  if (!NativeAudioContext) return;
  const contexts = new Set();
  window.AudioContext = new Proxy(NativeAudioContext, {
    construct(target, args) {
      const context = new target(...args);
      contexts.add(context);
      context.addEventListener('statechange', () => {
        if (context.state === 'closed') contexts.delete(context);
      });
      return context;
    },
  });
  const unlock = () => {
    for (const context of contexts) {
      if (context.state === 'suspended') context.resume().catch(() => {});
    }
  };
  window.addEventListener('pointerdown', unlock, { capture: true, passive: true });
  window.addEventListener('keydown', unlock, { capture: true });
})();
