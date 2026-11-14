"""usage: python3 -I serve.py <port> <dir> : threaded static server with a deep accept queue"""
import sys, functools, http.server
class S(http.server.ThreadingHTTPServer):
    request_queue_size = 256
    daemon_threads = True
h = functools.partial(http.server.SimpleHTTPRequestHandler, directory=sys.argv[2])
S(("127.0.0.1", int(sys.argv[1])), h).serve_forever()
