.PHONY: demo demo-down images images-amd64

# Levanta la demo local (construye para la arquitectura del host).
demo:
	docker compose up -d --build

demo-down:
	docker compose down

# Publica las imágenes multi-arquitectura (amd64 + arm64).
# Requiere: docker login ghcr.io
images:
	docker buildx bake --push

# Verificación local del build para x86-64 (QEMU).
images-amd64:
	docker buildx build --platform linux/amd64 -t cuylimeno-web:amd64 --load .
	docker buildx build --platform linux/amd64 -t cuylimeno-server:amd64 --load ./server
