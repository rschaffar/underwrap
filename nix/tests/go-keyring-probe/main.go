package main

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/zalando/go-keyring"
)

type secretFlags struct {
	service    string
	user       string
	secret     string
	secretStdin bool
}

type getFlags struct {
	service string
	user    string
}

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintf(os.Stderr, "underwrap-go-keyring-probe: %v\n", err)
		os.Exit(1)
	}
}

func run(args []string) error {
	if len(args) == 0 {
		return usageError("missing subcommand")
	}

	switch args[0] {
	case "set":
		cfg, err := parseSecretFlags("set", args[1:])
		if err != nil {
			return err
		}
		secret, err := cfg.resolveSecret()
		if err != nil {
			return err
		}
		return keyring.Set(cfg.service, cfg.user, secret)
	case "get":
		cfg, err := parseGetFlags("get", args[1:])
		if err != nil {
			return err
		}
		secret, err := keyring.Get(cfg.service, cfg.user)
		if err != nil {
			return err
		}
		fmt.Println(secret)
		return nil
	case "roundtrip":
		cfg, err := parseSecretFlags("roundtrip", args[1:])
		if err != nil {
			return err
		}
		secret, err := cfg.resolveSecret()
		if err != nil {
			return err
		}
		if err := keyring.Set(cfg.service, cfg.user, secret); err != nil {
			return fmt.Errorf("set user slot: %w", err)
		}
		actual, err := keyring.Get(cfg.service, cfg.user)
		if err != nil {
			return fmt.Errorf("get user slot: %w", err)
		}
		if actual != secret {
			return fmt.Errorf("user slot mismatch")
		}
		fmt.Println("user-slot=ok")
		return nil
	case "gh-login-sequence":
		cfg, err := parseSecretFlags("gh-login-sequence", args[1:])
		if err != nil {
			return err
		}
		secret, err := cfg.resolveSecret()
		if err != nil {
			return err
		}
		if err := keyring.Set(cfg.service, cfg.user, secret); err != nil {
			return fmt.Errorf("set user slot: %w", err)
		}
		userSecret, err := keyring.Get(cfg.service, cfg.user)
		if err != nil {
			return fmt.Errorf("get user slot: %w", err)
		}
		if userSecret != secret {
			return fmt.Errorf("user slot mismatch")
		}
		if err := keyring.Set(cfg.service, "", userSecret); err != nil {
			return fmt.Errorf("set active slot: %w", err)
		}
		activeSecret, err := keyring.Get(cfg.service, "")
		if err != nil {
			return fmt.Errorf("get active slot: %w", err)
		}
		if activeSecret != secret {
			return fmt.Errorf("active slot mismatch")
		}
		fmt.Println("user-slot=ok")
		fmt.Println("active-slot=ok")
		return nil
	default:
		return usageError(fmt.Sprintf("unknown subcommand: %s", args[0]))
	}
}

func parseSecretFlags(name string, args []string) (secretFlags, error) {
	fs := flag.NewFlagSet(name, flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	var cfg secretFlags
	fs.StringVar(&cfg.service, "service", "", "keyring service name")
	fs.StringVar(&cfg.user, "user", "", "keyring username")
	fs.StringVar(&cfg.secret, "secret", "", "secret material")
	fs.BoolVar(&cfg.secretStdin, "secret-stdin", false, "read secret from stdin")
	if err := fs.Parse(args); err != nil {
		return secretFlags{}, usageError(err.Error())
	}
	if cfg.service == "" || cfg.user == "" {
		return secretFlags{}, usageError("--service and --user are required")
	}
	return cfg, nil
}

func parseGetFlags(name string, args []string) (getFlags, error) {
	fs := flag.NewFlagSet(name, flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	var cfg getFlags
	fs.StringVar(&cfg.service, "service", "", "keyring service name")
	fs.StringVar(&cfg.user, "user", "", "keyring username")
	if err := fs.Parse(args); err != nil {
		return getFlags{}, usageError(err.Error())
	}
	if cfg.service == "" || cfg.user == "" {
		return getFlags{}, usageError("--service and --user are required")
	}
	return cfg, nil
}

func (cfg secretFlags) resolveSecret() (string, error) {
	if cfg.secret != "" && cfg.secretStdin {
		return "", usageError("specify only one of --secret or --secret-stdin")
	}
	if cfg.secret == "" && !cfg.secretStdin {
		return "", usageError("one of --secret or --secret-stdin is required")
	}
	if cfg.secret != "" {
		return cfg.secret, nil
	}
	secretBytes, err := io.ReadAll(os.Stdin)
	if err != nil {
		return "", fmt.Errorf("reading secret from stdin: %w", err)
	}
	secret := string(secretBytes)
	for len(secret) > 0 && (secret[len(secret)-1] == '\n' || secret[len(secret)-1] == '\r') {
		secret = secret[:len(secret)-1]
	}
	if secret == "" {
		return "", usageError("secret from stdin is empty")
	}
	return secret, nil
}

func usageError(msg string) error {
	return errors.New(msg)
}
