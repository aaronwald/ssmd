# cmd/ssmd/Dockerfile
FROM golang:1.23-alpine AS builder

WORKDIR /app

COPY go.mod go.sum ./
RUN go mod download

COPY . .
RUN CGO_ENABLED=0 go build -o ssmd ./cmd/ssmd

FROM alpine:3.19

RUN adduser -D -s /bin/false ssmd
COPY --from=builder /app/ssmd /usr/local/bin/

USER ssmd

ENTRYPOINT ["ssmd"]
CMD ["--help"]
