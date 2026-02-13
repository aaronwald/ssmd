// CLI failure email notification
import nodemailer from "npm:nodemailer@6";

/**
 * Send an email notification when a CLI command fails.
 * Reads SMTP config from environment variables.
 */
export async function sendFailureEmail(
  command: string,
  error: Error,
): Promise<void> {
  const host = Deno.env.get("SMTP_HOST") ?? "smtp.gmail.com";
  const port = Number(Deno.env.get("SMTP_PORT") ?? "587");
  const user = Deno.env.get("SMTP_USER");
  const pass = Deno.env.get("SMTP_PASS");
  const to = Deno.env.get("SMTP_TO");

  if (!user || !pass || !to) {
    console.error("[notify] SMTP_USER, SMTP_PASS, and SMTP_TO required for email notification");
    return;
  }

  const transporter = nodemailer.createTransport({
    host,
    port,
    secure: false,
    auth: { user, pass },
  });

  const body = [
    `Command: ssmd ${command}`,
    `Error: ${error.message}`,
    "",
    "Stack trace:",
    error.stack ?? "(no stack trace)",
  ].join("\n");

  await transporter.sendMail({
    from: user,
    to,
    subject: `[SSMD] CLI Failed: ${command}`,
    text: body,
  });

  console.log(`[notify] Failure email sent to ${to}`);
}
