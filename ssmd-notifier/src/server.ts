// ssmd-notifier/src/server.ts

export interface Metrics {
  firesReceived: number;
  notificationsSent: number;
  notificationsFailed: number;
}

const metrics: Metrics = {
  firesReceived: 0,
  notificationsSent: 0,
  notificationsFailed: 0,
};

export function getMetrics(): Metrics {
  return { ...metrics };
}

export function incrementFiresReceived(): void {
  metrics.firesReceived++;
}

export function incrementNotificationsSent(): void {
  metrics.notificationsSent++;
}

export function incrementNotificationsFailed(): void {
  metrics.notificationsFailed++;
}

/**
 * Start HTTP server for health checks and metrics.
 */
export function startServer(port: number = 9090): void {
  Deno.serve({ port }, (req) => {
    const url = new URL(req.url);

    switch (url.pathname) {
      case "/health":
        return new Response("ok", { status: 200 });

      case "/ready":
        return new Response("ok", { status: 200 });

      case "/metrics": {
        const m = getMetrics();
        const body = [
          `# HELP ssmd_notifier_fires_received Total signal fires received`,
          `# TYPE ssmd_notifier_fires_received counter`,
          `ssmd_notifier_fires_received ${m.firesReceived}`,
          `# HELP ssmd_notifier_notifications_sent Total notifications sent`,
          `# TYPE ssmd_notifier_notifications_sent counter`,
          `ssmd_notifier_notifications_sent ${m.notificationsSent}`,
          `# HELP ssmd_notifier_notifications_failed Total notifications failed`,
          `# TYPE ssmd_notifier_notifications_failed counter`,
          `ssmd_notifier_notifications_failed ${m.notificationsFailed}`,
        ].join("\n");
        return new Response(body, {
          status: 200,
          headers: { "Content-Type": "text/plain" },
        });
      }

      default:
        return new Response("Not Found", { status: 404 });
    }
  });

  console.log(`Health server listening on :${port}`);
}
