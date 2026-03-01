"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";

/** Legacy /select route — redirects to /settings */
export default function SelectRedirect() {
  const router = useRouter();
  useEffect(() => { router.replace("/settings"); }, [router]);
  return null;
}
