import { NextRequest, NextResponse } from 'next/server';
import crypto from 'crypto';

const COOKIE_NAME = 'mercy_session';

function verifySession(cookieValue: string, secret: string): boolean {
  const lastDot = cookieValue.lastIndexOf('.');
  if (lastDot === -1) return false;

  const value = cookieValue.substring(0, lastDot);
  const providedSig = cookieValue.substring(lastDot + 1);

  const hmac = crypto.createHmac('sha256', secret);
  hmac.update(value);
  const expectedSig = hmac.digest('base64url');

  return providedSig === expectedSig;
}

export function proxy(request: NextRequest) {
  const { pathname } = request.nextUrl;

  if (!pathname.startsWith('/dashboard') && !pathname.startsWith('/api/proxy')) {
    return NextResponse.next();
  }

  const cookie = request.cookies.get(COOKIE_NAME);
  const secret = process.env.MERCY_SESSION_SECRET || 'insecure-dev-secret';

  if (!cookie || !verifySession(cookie.value, secret)) {
    if (pathname.startsWith('/api/')) {
      return NextResponse.json({ error: 'unauthorized' }, { status: 401 });
    }
    return NextResponse.redirect(new URL('/login', request.url));
  }

  return NextResponse.next();
}

export const config = {
  matcher: ['/dashboard/:path*', '/api/proxy/:path*'],
};
