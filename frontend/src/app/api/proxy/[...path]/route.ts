import { NextRequest, NextResponse } from 'next/server';

const BACKEND_URL = process.env.MERCY_BACKEND_URL || 'http://127.0.0.1:8090';
const AUTH_TOKEN = process.env.MERCY_AUTH_TOKEN || '';

async function proxyRequest(request: NextRequest, params: Promise<{ path: string[] }>) {
  const { path } = await params;
  const backendPath = path.join('/');
  const url = new URL(`/${backendPath}`, BACKEND_URL);

  // Forward query params
  request.nextUrl.searchParams.forEach((value, key) => {
    url.searchParams.set(key, value);
  });

  const headers: Record<string, string> = {
    'Authorization': `Bearer ${AUTH_TOKEN}`,
  };

  // Forward content-type for POST requests
  const contentType = request.headers.get('content-type');
  if (contentType) {
    headers['Content-Type'] = contentType;
  }

  const fetchOptions: RequestInit = {
    method: request.method,
    headers,
  };

  if (request.method !== 'GET' && request.method !== 'HEAD') {
    fetchOptions.body = await request.text();
  }

  try {
    const backendRes = await fetch(url.toString(), fetchOptions);

    const responseHeaders = new Headers();
    const ct = backendRes.headers.get('content-type');
    if (ct) responseHeaders.set('content-type', ct);
    const cd = backendRes.headers.get('content-disposition');
    if (cd) responseHeaders.set('content-disposition', cd);

    return new NextResponse(backendRes.body, {
      status: backendRes.status,
      headers: responseHeaders,
    });
  } catch (err) {
    return NextResponse.json(
      { error: 'Backend unavailable', detail: String(err) },
      { status: 502 }
    );
  }
}

export async function GET(request: NextRequest, context: { params: Promise<{ path: string[] }> }) {
  return proxyRequest(request, context.params);
}

export async function POST(request: NextRequest, context: { params: Promise<{ path: string[] }> }) {
  return proxyRequest(request, context.params);
}
