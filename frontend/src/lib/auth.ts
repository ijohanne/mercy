import { cookies } from 'next/headers';
import crypto from 'crypto';

const COOKIE_NAME = 'mercy_session';
const SESSION_SECRET = process.env.MERCY_SESSION_SECRET || 'insecure-dev-secret';

function sign(value: string): string {
  const hmac = crypto.createHmac('sha256', SESSION_SECRET);
  hmac.update(value);
  const signature = hmac.digest('base64url');
  return `${value}.${signature}`;
}

function verify(signed: string): string | null {
  const lastDot = signed.lastIndexOf('.');
  if (lastDot === -1) return null;

  const value = signed.substring(0, lastDot);
  const expected = sign(value);
  if (signed !== expected) return null;
  return value;
}

export async function getSession(): Promise<string | null> {
  const cookieStore = await cookies();
  const cookie = cookieStore.get(COOKIE_NAME);
  if (!cookie) return null;
  return verify(cookie.value);
}

export function createSessionCookie(username: string): { name: string; value: string; options: Record<string, unknown> } {
  return {
    name: COOKIE_NAME,
    value: sign(username),
    options: {
      httpOnly: true,
      secure: process.env.NODE_ENV === 'production',
      sameSite: 'lax' as const,
      path: '/',
      maxAge: 60 * 60 * 24 * 7, // 7 days
    },
  };
}

export function clearSessionCookie(): { name: string; value: string; options: Record<string, unknown> } {
  return {
    name: COOKIE_NAME,
    value: '',
    options: {
      httpOnly: true,
      secure: process.env.NODE_ENV === 'production',
      sameSite: 'lax' as const,
      path: '/',
      maxAge: 0,
    },
  };
}
