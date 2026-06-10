import nodemailer from "npm:nodemailer@6";

export interface WelcomeMeta {
  recipient: string;
  link: string;
  apiBaseUrl: string;
  feeds: string[];
  dateFrom: string;
  dateTo: string;
  ttlDays: number;
  /** Only used to assert it is NOT leaked; never rendered. */
  rawSecret?: string;
}

export function composeWelcomeEmail(m: WelcomeMeta): { subject: string; text: string } {
  const subject = "Your Varshtat data API key";
  const text = [
    `Welcome — you've been granted access to the Varshtat market-data API.`,
    ``,
    `Retrieve your API key here (one-time link, expires in ${m.ttlDays} days):`,
    `  ${m.link}`,
    ``,
    `Once you open it, the key is shown ONCE — copy it somewhere safe.`,
    ``,
    `API base URL: ${m.apiBaseUrl}`,
    `Allowed feeds: ${m.feeds.join(", ")}`,
    `Date range: ${m.dateFrom} to ${m.dateTo}`,
    ``,
    `Or download via the website:`,
    `  1. Log in at https://harman.varshtat.com with your Google account (${m.recipient}).`,
    `  2. Open "Files", pick a date, and download (or copy the download script).`,
    `  (Website access uses Google sign-in and must be enabled for your email.)`,
    ``,
    `Quickstart: see researcher-quickstart in the ssmd docs.`,
    `Example:`,
    `  curl -H "X-API-Key: <your-key>" "${m.apiBaseUrl}/v1/data/download?feed=hols&from=2026-06-01&to=2026-06-01"`,
  ].join("\n");
  return { subject, text };
}

/** Send the welcome email via the existing SMTP config. Throws on failure. */
export async function sendWelcomeEmail(m: WelcomeMeta): Promise<void> {
  const host = Deno.env.get("SMTP_HOST") ?? "smtp.gmail.com";
  const port = Number(Deno.env.get("SMTP_PORT") ?? "587");
  const user = Deno.env.get("SMTP_USER");
  const pass = Deno.env.get("SMTP_PASS");
  if (!user || !pass) throw new Error("SMTP_USER and SMTP_PASS required to send welcome email");

  const { subject, text } = composeWelcomeEmail(m);
  const transporter = nodemailer.createTransport({ host, port, secure: false, auth: { user, pass } });
  await transporter.sendMail({ from: user, to: m.recipient, subject, text });
}
