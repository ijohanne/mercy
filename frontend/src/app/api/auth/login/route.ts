import { NextRequest, NextResponse } from 'next/server';
import { createSessionCookie } from '@/lib/auth';

export async function POST(request: NextRequest) {
  const body = await request.json();
  const { username, password } = body;

  const expectedUser = process.env.MERCY_ADMIN_USER || 'admin';
  const expectedPass = process.env.MERCY_ADMIN_PASSWORD;

  if (!expectedPass) {
    return NextResponse.json(
      { error: 'MERCY_ADMIN_PASSWORD not configured' },
      { status: 500 }
    );
  }

  if (username !== expectedUser || password !== expectedPass) {
    return NextResponse.json({ error: 'Invalid credentials' }, { status: 401 });
  }

  const { name, value, options } = createSessionCookie(username);
  const response = NextResponse.json({ ok: true });
  response.cookies.set(name, value, options);
  return response;
}
